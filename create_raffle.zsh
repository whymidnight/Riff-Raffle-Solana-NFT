./target/debug/draffle create-raffle \
    --name $1 \
    EE5qLwD5zh2w27cvGceW9dco6QUrsgNrz24YaHLxxVpn \
    1000 \
    --end-date "$(gdate --utc -d "+5 minute" '+%Y-%m-%d %H:%M')" \
    --max-entrants 690 \
    --provider.cluster devnet \
    --provider.wallet oracle.key \
    --program-id 2gPKWB9obxygRWPxEbA1ZHexUsQNHMH7EZ71WER8cc61
