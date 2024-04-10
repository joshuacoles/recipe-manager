# Instagram Recipe Manager

The people in my life keep sending me instagram reels with very nice recipes in them but for the life of me I can't cook
from them. This is a simple project to help me manage those recipes. It

- Downloads them from instagram using youtube-dlp
- Passes the description through an LLM to get the ingredients, steps, and title
- Saves the recipe into a database for easy access

This is also a place for me to play with a personal stack for rust based webapp, specifically Fang for managing
background jobs and HTMX for the frontend.

## Future Features

- Whisper on the video audio for more clear instructions.
  - Include transcription as raw block as well as feed into the LLM.

## Todo Before Deployment

- Fix model calling
- Allow for whisper.cpp
  - Probably want to make a service running on `beachhead` for this
