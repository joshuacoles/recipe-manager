alter table recipes
    add column generated_at timestamp with time zone default current_timestamp;
