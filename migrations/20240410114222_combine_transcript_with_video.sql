alter table public.instagram_video
    add column transcript jsonb;

insert into instagram_video (id, transcript)
select iv.id, t.json
from public.instagram_video iv
         join public.transcript t on t.id = iv.transcript_id
on conflict (id) do update set transcript = excluded.transcript;

alter table public.instagram_video
    drop column transcript_id;

drop table public.transcript;
