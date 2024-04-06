-- Make instagram_id unique in unprocessed_recipes
alter table unprocessed_recipes
    add unique (instagram_id);

alter table recipes
    add unique (instagram_id);
