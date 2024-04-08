alter table recipes
    alter column instructions drop not null,
    alter column ingredients drop not null,
    alter column title drop not null,
    drop column raw_description;

insert into recipes (instagram_video_id)
select instagram_video_id
from unprocessed_recipes
where instagram_video_id is not null;

drop table unprocessed_recipes;
