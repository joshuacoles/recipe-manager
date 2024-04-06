create table recipes
(
    id              serial primary key,
    instagram_id    text   not null,
    title           text   not null,
    raw_description text   not null,
    ingredients     text[] not null,
    instructions    text[] not null
);
