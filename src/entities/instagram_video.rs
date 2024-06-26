//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.2

use crate::jobs::extract_transcript::Transcript;
use crate::jobs::fetch_reel::ReelInfo;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "instagram_video")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(column_type = "Text", unique)]
    pub instagram_id: String,
    #[sea_orm(column_type = "Text")]
    pub video_url: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub info: ReelInfo,
    pub created_at: Option<DateTime>,
    pub updated_at: Option<DateTime>,
    #[sea_orm(column_type = "JsonBinary", nullable)]
    pub transcript: Option<Transcript>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::recipes::Entity")]
    Recipes,
}

impl Related<super::recipes::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Recipes.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
