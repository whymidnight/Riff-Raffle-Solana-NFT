URI="https://raw.githubusercontent.com/not-bored-ape/images/master/unknown.jpeg"
cargo run create-raffle \
    --name "$NAME" \
    --image-uri "$URI" \
    --proceeds-mint EE5qLwD5zh2w27cvGceW9dco6QUrsgNrz24YaHLxxVpn \
    --ticket-price 1000 \
    --end-date "$(gdate --utc -d "+10 minute" '+%Y-%m-%d %H:%M')" \
    --max-entrants 690 \
    --provider.cluster devnet \
    --provider.wallet oracle.key \
    --program-id 2gPKWB9obxygRWPxEbA1ZHexUsQNHMH7EZ71WER8cc61