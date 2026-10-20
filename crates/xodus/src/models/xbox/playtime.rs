use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserStatsBatchRequest {
    #[serde(rename = "arrangebyfield")]
    pub arrange_by_field: String,
    pub xuids: Vec<String>,
    pub stats: Vec<TitleName>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TitleName {
    pub name: String,
    pub title_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct UserStatsBatchResponse {
    pub stat_lists_collection: Vec<StatlistCollection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatlistCollection {
    #[serde(rename = "arrangebyfield")]
    pub arrange_by_field: String,
    #[serde(rename = "arrangebyfieldid")]
    pub arrange_by_field_id: String,
    pub stats: Vec<StatsBody>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsBody {
    #[serde(rename = "titleid")]
    pub title_id: String,
    pub name: String,
    pub value: String,
}
