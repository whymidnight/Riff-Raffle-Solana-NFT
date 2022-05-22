# $1 - raffle_account
# $2 - token mint
# $3 - token prize amount
# $4 - prize index

./target/debug/draffle reveal-winners \
    $1 \
    --provider.cluster devnet \
    --provider.wallet oracle.key \
    --program-id 2gPKWB9obxygRWPxEbA1ZHexUsQNHMH7EZ71WER8cc61
