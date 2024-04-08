alter table transcript drop column segments;

-- No need for default, we haven't generated any transcripts yet
alter table transcript add column content text not null;
