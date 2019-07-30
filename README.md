# mc-sync

This Discord bot wraps around a Minecraft server process in order to integrate with a Discord server.

There are three concurrent threads:

- Read commands from stdin and forward them to the inner Minecraft server's stdin.

- Read output from the inner server's stdout, parse them for interesting 
  events (e.g. players logging in), and send them to Discord.

- Listen to messages from Discord and broadcast them within 
  Minecraft by writing a `/say` command to the inner Minecraft server.

Currently the bot also provides an `!online` command for listing the players currently logged into
the server.

## Usage

Expects three environment variables to be set:

- `DISCORD_TOKEN` is this bot's application token. Private for now.
- `GENERAL_CHANNEL` is the Discord channel ID where this bot forwards interesting server events.
- `VERBOSE_CHANNEL` is the Discord channel ID where this bot forwards all server logs.

Run the bot with the server command as its first argument. For example,

```bash
#!/bin/sh
# start.sh

java -Xmx3024M -Xms3024M -jar server.jar nogui 
```

This may differ depending on your directory structure:

```bash
> cargo build --release
> ./target/release/mc-sync "../server/start.sh"
```

## Screenshot

![screenshot](assets/screenshot.jpg)
