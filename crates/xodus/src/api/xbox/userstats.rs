use crate::models::xbox::playtime::{ UserStatsBatchRequest, UserStatsBatchResponse, TitleName };

pub async fn get_user_playtime (
    client: &reqwest::Client,
    auth_header: &str,
    xuid: &str,
    title_ids: &[u32],

) -> reqwest::Result<UserStatsBatchResponse> {

    let body = UserStatsBatchRequest {
        arrange_by_field: "xuid".to_string(),
        xuids: vec![xuid.to_string()],
        stats: title_ids
            .iter()
            .map(|&id| TitleName {
                name: "MinutesPlayed".to_string(),
                title_id: id.to_string(),
            })
            .collect(),
    };

    let response = client
        .post("https://userstats.xboxlive.com/batch")
        .header("x-xbl-contract-version", "2")
        .header("Authorization", auth_header)
        .json(&body)
        .send()
        .await?;

    response.error_for_status()?.json().await
}