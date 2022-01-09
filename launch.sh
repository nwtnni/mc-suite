#!/bin/sh

export DISCORD_TOKEN="<REDACTED>"
export DISCORD_GENERAL_CHANNEL_ID="<REDACTED>"
export DISCORD_VERBOSE_CHANNEL_ID="<REDACTED>"

# Assumes the following directory layout:
#
# /home/ec2-user
# - mc-suite
# - Minecraft-Overviewer
# - render
# - vanilla

/home/ec2-user/mc-suite/target/release/mc-sync /home/ec2-user/mc-suite/minecraft.sh
/home/ec2-user/Minecraft-Overviewer/overviewer.py --config /home/ec2-user/Minecraft-Overviewer/config.py
rclone sync -L /home/ec2-user/render s3:craft.nwtnni.me
sudo shutdown now
