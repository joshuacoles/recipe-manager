create table instagram_video
(
    id           serial primary key,
    instagram_id text  not null unique,
    video_url    text  not null,
    info         jsonb not null,

    created_at   timestamp default now(),
    updated_at   timestamp default now()
);

SELECT diesel_manage_updated_at('instagram_video');

insert into instagram_video (instagram_id, video_url, info)
select instagram_id, instagram_url, info_json
from recipes
on conflict do nothing;

insert into instagram_video (instagram_id, video_url, info)
select instagram_id, instagram_url, info_json
from unprocessed_recipes
on conflict do nothing;

-- Update recipes

alter table recipes
    add column instagram_video_id integer references instagram_video (id);

update recipes
set instagram_video_id = instagram_video.id
from instagram_video
where recipes.instagram_id = instagram_video.instagram_id;

alter table recipes
    drop column instagram_id,
    drop column instagram_url,
    drop column info_json;

-- Update unprocessed_recipes

alter table unprocessed_recipes
    add column instagram_video_id integer references instagram_video (id);

update unprocessed_recipes
set instagram_video_id = instagram_video.id
from instagram_video
where unprocessed_recipes.instagram_id = instagram_video.instagram_id;

alter table unprocessed_recipes
    drop column instagram_id,
    drop column instagram_url,
    drop column info_json;

