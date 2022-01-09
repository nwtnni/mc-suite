#!/bin/sh

# Launch script from `tmux` session, so we can SSH in and attach
# for debugging.
#
# This script runs on startup via the following cron expression,
# which goes into `crontab -e`:
#
# ```
# @reboot /home/ec2-user/mc-suite/launch-tmux.sh
# ```

tmux new-session -d -s mc /home/ec2-user/mc-suite/launch.sh
