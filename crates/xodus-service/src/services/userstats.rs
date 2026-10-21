use std::error::Error;

use tokio::sync::OnceCell;

use xodus_peer::proto::xodus::userstats::*;

use xodus::api::xbox::title::{get_endpoint, get_title_management};
use xodus::api::xbox::userstats::get_userstats;
use xodus::api::xbox::{get_xsts_auth_header, run};
use xodus::models::secrets::Token;
use xodus::tokens::TokenManager;

pub struct Userstats {
    client: reqwest::Client,
    tokens: TokenManager,
    relying_party: OnceCell<String>,
}

impl Userstats {
    pub fn new(client: &reqwest::Client, tokens: &TokenManager) -> Self {
        Self {
            client: client.clone(),
            tokens: tokens.clone(),
            relying_party: OnceCell::new(),
        }
    }

    async fn get_relying_party(&self) -> Result<&str, Box<dyn Error + Send + Sync>> {
        let url = self
            .relying_party
            .get_or_try_init(|| async {
                let title_mgt_response = get_title_management(&self.client).await?;
                let relying_party =
                    get_endpoint("https://userstats.xboxlive.com", &title_mgt_response)
                        .and_then(|e| e.relying_party.clone())
                        .ok_or("no relying party for host")?;
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(relying_party)
            })
            .await?;
        Ok(url)
    }
}

impl UserstatsService for Userstats {
    async fn get_userstats(
        &self,
        req: GetUserstatsRequest,
    ) -> Result<GetUserstatsResponse, Box<dyn Error + Send + Sync>> {
        if req.xuid == 0 {
            return Ok(GetUserstatsResponse {
                titles: Vec::new(),
                error: UserstatsError::NotSignedIn as i32,
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

                let tok = run(&self.client, dev_token, legacy, relying_party).await;
                self.tokens.cache_xsts(relying_party, &tok);
                tok
            }
        };

        let parsed = get_userstats(
            &self.client,
            &get_xsts_auth_header(xsts_token),
            &req.xuid.to_string(),
            &req.title_ids,
        )
        .await?;

        let titles = parsed
            .statlistscollection
            .into_iter()
            .flat_map(|collection| {
                collection.stats.into_iter().filter_map(|stat| {
                    Some(TitleStats {
                        title_id: stat.title_id.parse().ok()?,
                        minutes_played: stat.value.parse().ok()?,
                    })
                })
            })
            .collect();

        Ok(GetUserstatsResponse {
            titles,
            error: UserstatsError::Ok as i32,
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

        let svc = Userstats::new(&client, &tokens);
        let resp = svc
            .get_userstats(GetUserstatsRequest {
                xuid: 0,
                title_ids: vec![1705097731],
            })
            .await
            .unwrap();
        assert_eq!(resp.error, UserstatsError::NotSignedIn as i32);
        assert!(resp.titles.is_empty());
    }

    #[ignore]
    #[tokio::test]
    async fn test_get_userstats() {
        let client = reqwest::Client::new();
        xodus::secrets::init_secrets().expect("Unable to initialize credentials");
        let tokens = TokenManager::with_keychain_and_memory();
        xodus::tokens::device::ensure_device_credentials(&client, &tokens).await;

        let svc = Userstats::new(&client, &tokens);
        let xuid = std::env::var("XUID").expect("set XUID").parse().unwrap();
        let resp = svc
            .get_userstats(GetUserstatsRequest {
                xuid,
                title_ids: vec![1705097731, 1810111157],
            })
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
        let svc = Userstats::new(&client, &tokens);
        let rp = svc.get_relying_party().await.unwrap();
        assert!(tokens.get_cached_xsts(rp).is_none());
        let xuid = std::env::var("XUID").expect("set XUID").parse().unwrap();
        let resp = svc
            .get_userstats(GetUserstatsRequest {
                xuid,
                title_ids: vec![1705097731],
            })
            .await
            .unwrap();
        assert!(tokens.get_cached_xsts(rp).is_some());
        println!("{resp:?}");
        let resp = svc
            .get_userstats(GetUserstatsRequest {
                xuid,
                title_ids: vec![1810111157],
            })
            .await
            .unwrap();
        assert_eq!(resp.error, UserstatsError::Ok as i32);
        println!("{resp:?}");
    }
}
