tailwind:
    /opt/homebrew/Cellar/tailwindcss/3.4.3/bin/tailwindcss -i ./app/stylesheets/application.css -o ./public/stylesheets/application.css

tailwind-watch:
    /opt/homebrew/Cellar/tailwindcss/3.4.3/bin/tailwindcss -i ./app/stylesheets/application.css -o ./public/stylesheets/application.css --watch

generate-entities:
    sea-orm-cli generate entity --database-url postgres://postgres@localhost/recipes \
                                --with-serde both \
                                --ignore-tables _sqlx_migrations,fang_tasks \
                                --output-dir ./src/entities
