//! `SeaORM` Entity. Generated by sea-orm-codegen 0.12.2

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq, Serialize, Deserialize)]
#[sea_orm(table_name = "recipes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(column_type = "Text", nullable)]
    pub title: Option<String>,
    pub ingredients: Option<Vec<String>>,
    pub instructions: Option<Vec<String>>,
    pub updated_at: DateTimeWithTimeZone,
    pub instagram_video_id: Option<i32>,
    pub generated_at: Option<DateTimeWithTimeZone>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::instagram_video::Entity",
        from = "Column::InstagramVideoId",
        to = "super::instagram_video::Column::Id",
        on_update = "NoAction",
        on_delete = "NoAction"
    )]
    InstagramVideo,
}

impl Related<super::instagram_video::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::InstagramVideo.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
