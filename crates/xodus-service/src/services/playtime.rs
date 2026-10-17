use std::collections::HashMap;
use std::error::Error;
use std::sync::Mutex;
use tokio::sync::OnceCell;

use tokio_util::sync::CancellationToken;
use xodus_peer::proto::xodus::playtime::*;
use xodus::api::xbox::userstats::get_user_playtime;

use xodus::api::xbox::title::{get_endpoint, get_title_management};
use xodus::api::xbox::{get_xsts_auth_header, run};
use xodus::models::secrets::Token;
use xodus::tokens::TokenManager;

pub struct Playtime {
    sessions: Mutex<Sessions>,
    client: reqwest::Client,
    tokens: TokenManager,
    relying_party: OnceCell<String>,
}

struct SessionState {
    xuid: u64,
    title_id: u32,
    started_at: i64,
    last_heartbeat_at: i64,
    last_heartbeat_ok: bool,
    cancel: CancellationToken,
}

#[derive(Default)]
struct Sessions {
    next_id: u32,
    by_id: HashMap<u32, SessionState>,
}

impl Playtime {
    pub fn new(client: &reqwest::Client, tokens: &TokenManager) -> Self {
        Self {
            sessions: Mutex::new(Sessions::default()),
            client: client.clone(),
            tokens: tokens.clone(),
            relying_party: OnceCell::new(),
        }
    }

    async fn get_relying_party(&self) -> Result<&str, Box<dyn std::error::Error + Send + Sync>> {
        let url = self
            .relying_party
            .get_or_try_init(|| async {
                let title_mgt_response = get_title_management(&self.client).await?;
                let relying_party = get_endpoint("https://userstats.xboxlive.com", &title_mgt_response)
                    .and_then(|e| e.relying_party.clone())
                    .ok_or("no relying party for host")?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(relying_party)
            })
            .await?;
        Ok(url)
    }
}

impl PlaytimeService for Playtime {
    async fn get_playtime(
        &self,
        req: GetPlaytimeRequest,
    ) -> Result<GetPlaytimeResponse, Box<dyn Error + Send + Sync>> {
        
        if req.xuid == 0 {
            return Ok(GetPlaytimeResponse {
                titles: Vec::new(),
                error: PlaytimeError::NotSignedIn as i32,
            });
        }

        let relying_party = self.get_relying_party().await?;

        let xsts_token = match self.tokens.get_cached_xsts(relying_party) {
            Some(tok) => tok,
            None => {

                let dev_token = self.tokens.get_device_sts_token()?;
                let Token::Legacy(dev_token) = dev_token else {
                    return Err("device STS token is not a legacy token".into());
                };

                let user_token = self.tokens.get_user_sts_token()?;
                let Token::Legacy(legacy) = user_token else {
                    return Err("user STS token is not a legacy token".into());
                };

                let tok = run(&self.client, dev_token, legacy, &relying_party).await;
                self.tokens.cache_xsts(relying_party, &tok);
                tok
            }
        };

        let parsed = get_user_playtime(&self.client, &get_xsts_auth_header(xsts_token), &req.xuid.to_string(), &req.title_ids).await?;

        let titles = parsed
            .statlistscollection
            .into_iter()
            .flat_map(|collection| {
                collection.stats.into_iter().filter_map(|stat| {
                    Some(TitlePlaytime {
                        title_id: stat.title_id.parse().ok()?,
                        minutes_played: stat.value.parse().ok()?,
                    })
                })
            })
            .collect();

        Ok(GetPlaytimeResponse {
            titles,
            error: PlaytimeError::Ok as i32,
        })
    }

    async fn list_active_sessions(
        &self,
        _req: ListActiveSessionsRequest,
    ) -> Result<ListActiveSessionsResponse, Box<dyn Error + Send + Sync>> {
        let mut active_sessions = Vec::new();
        for (id, session) in self.sessions.lock().unwrap().by_id.iter() {
            active_sessions.push(ActiveSession {
                session_id: *id,
                xuid: session.xuid,
                title_id: session.title_id,
                started_at: session.started_at,
                last_heartbeat_at: session.last_heartbeat_at,
                last_heartbeat_ok: session.last_heartbeat_ok,
            });
        }

        Ok(ListActiveSessionsResponse {
            sessions: active_sessions,
        })
    }

    async fn title_started(
        &self,
        req: TitleStartedRequest,
    ) -> Result<TitleStartedResponse, Box<dyn Error + Send + Sync>> {
        // Creating sessions

        if req.xuid == 0 {
            return Ok(TitleStartedResponse {
                session_id: 0,
                error: PlaytimeError::NotSignedIn as i32,
            });
        }

        if req.title_id == 0 {
            return Ok(TitleStartedResponse {
                session_id: 0,
                error: PlaytimeError::InvalidArgument as i32,
            });
        }

        let id = {
            let mut s = self.sessions.lock().unwrap();

            if s.by_id
                .values()
                .any(|session| session.xuid == req.xuid && session.title_id == req.title_id)
            {
                return Ok(TitleStartedResponse {
                    session_id: 0,
                    error: PlaytimeError::AlreadyRunning as i32,
                });
            }

            s.next_id += 1;
            let id = s.next_id;
            s.by_id.insert(
                id,
                SessionState {
                    xuid: req.xuid,
                    title_id: req.title_id,
                    cancel: CancellationToken::new(),
                    started_at: chrono::Utc::now().timestamp(),
                    last_heartbeat_at: 0,
                    last_heartbeat_ok: false,
                },
            );
            id
        };

        Ok(TitleStartedResponse {
            session_id: id,
            error: PlaytimeError::Ok as i32,
        })
    }

    async fn title_stopped(
        &self,
        req: TitleStoppedRequest,
    ) -> Result<TitleStoppedResponse, Box<dyn Error + Send + Sync>> {
        // Ending sessions
        {
            let mut sessions = self.sessions.lock().unwrap();
            if let Some(session) = sessions.by_id.remove(&req.session_id) {
                session.cancel.cancel();
            } else {
                return Ok(TitleStoppedResponse {
                    error: PlaytimeError::UnknownSession as i32,
                });
            }
        };

        Ok(TitleStoppedResponse {
            error: PlaytimeError::Ok as i32,
        })
    }

    
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_not_signed_in() {
        let client = reqwest::Client::new();
        let tokens = TokenManager::with_memory();
        xodus::tokens::device::ensure_device_credentials(&client, &tokens).await;

        let svc = Playtime::new(&client, &tokens);
        let resp = svc
            .get_playtime(GetPlaytimeRequest { xuid: 0, title_ids: vec![1705097731] })
            .await
            .unwrap();
        assert_eq!(resp.error, PlaytimeError::NotSignedIn as i32);
        assert!(resp.titles.is_empty());
    }

    #[ignore]
    #[tokio::test]
    async fn test_get_playtime() {
        let client = reqwest::Client::new();
        xodus::secrets::init_secrets().expect("Unable to initialize credentials");
        let tokens = TokenManager::with_keychain_and_memory();
        xodus::tokens::device::ensure_device_credentials(&client, &tokens).await;

        let svc = Playtime::new(&client, &tokens);
        let xuid = std::env::var("XUID").expect("set XUID").parse().unwrap();
        let resp = svc
            .get_playtime(GetPlaytimeRequest { xuid, title_ids: vec![1705097731, 1810111157] })
            .await
            .unwrap();
        println!("{resp:?}");
    }

    #[ignore]
    #[tokio::test]
    async fn test_cache_token() {
        let client = reqwest::Client::new();
        xodus::secrets::init_secrets().expect("Unable to initialize credentials");
        let tokens = TokenManager::with_keychain_and_memory();
        xodus::tokens::device::ensure_device_credentials(&client, &tokens).await;
        
        let svc = Playtime::new(&client, &tokens);
        let rp = svc.get_relying_party().await.unwrap();
        assert!(tokens.get_cached_xsts(rp).is_none());
        let xuid = std::env::var("XUID").expect("set XUID").parse().unwrap();
        let resp = svc
            .get_playtime(GetPlaytimeRequest { xuid, title_ids: vec![1705097731] })
            .await
            .unwrap();
        assert!(tokens.get_cached_xsts(rp).is_some());
        println!("{resp:?}");
        let resp = svc
            .get_playtime(GetPlaytimeRequest { xuid, title_ids: vec![1810111157] })
            .await
            .unwrap();
        assert_eq!(resp.error, PlaytimeError::Ok as i32);
        println!("{resp:?}");
    }
}
