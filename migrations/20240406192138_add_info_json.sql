alter table recipes
    add column info_json jsonb;

alter table recipes
    add column instagram_url text;

alter table recipes
    add column updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW();

create table
    unprocessed_recipes
(
    id            serial primary key,
    instagram_id  text                     not null,
    instagram_url text                     not null,
    info_json     jsonb                    not null,
    created_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

SELECT diesel_manage_updated_at('recipes');
SELECT diesel_manage_updated_at('unprocessed_recipes');
