alter table instagram_video
    add column transcript_id integer references transcript (id);

update instagram_video
set transcript_id = r.transcript_id
from recipes r
where instagram_video.id = r.instagram_video_id;

alter table recipes
    drop column transcript_id;
