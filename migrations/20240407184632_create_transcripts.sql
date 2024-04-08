create table transcript
(
    id serial primary key,
    segments text[] not null
);

alter table recipes
    add column transcript_id integer references transcript(id);
