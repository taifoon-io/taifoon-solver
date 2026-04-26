#!/opt/homebrew/bin/bash
#
# Extract fixture data from 100 live intents for testing and validation
# Purpose: Create test fixtures for each protocol from live production data
#

set -e

FIXTURES_DIR="fixtures"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)

echo "═══════════════════════════════════════════════════════════════════════"
echo "  TAIFOON SOLVER INTENT FIXTURE EXTRACTOR"
echo "═══════════════════════════════════════════════════════════════════════"
echo

# Create fixtures directory
mkdir -p "$FIXTURES_DIR"

# Fetch all intents
echo "Fetching 100 live intents from solver..."
INTENTS_JSON=$(curl -s "http://localhost:8082/api/solver/intents")
INTENT_COUNT=$(echo "$INTENTS_JSON" | jq '.intents | length')

echo "✓ Found $INTENT_COUNT intents"
echo

# Save full dump
FULL_DUMP_FILE="$FIXTURES_DIR/intents-full-dump-$TIMESTAMP.json"
echo "$INTENTS_JSON" > "$FULL_DUMP_FILE"
echo "✓ Saved full dump to: $FULL_DUMP_FILE"
echo

# Extract unique protocols
PROTOCOLS=$(echo "$INTENTS_JSON" | jq -r '.intents[].protocol' | sort -u)
PROTOCOL_COUNT=$(echo "$PROTOCOLS" | wc -l | tr -d ' ')

echo "Found $PROTOCOL_COUNT unique protocols:"
echo "$PROTOCOLS" | while read -r protocol; do
    count=$(echo "$INTENTS_JSON" | jq "[.intents[] | select(.protocol == \"$protocol\")] | length")
    echo "  - $protocol: $count intents"
done
echo

# Extract fixture samples per protocol
echo "Extracting per-protocol fixtures..."
echo

for protocol in $PROTOCOLS; do
    safe_name=$(echo "$protocol" | tr '/' '_' | tr ' ' '_')
    fixture_file="$FIXTURES_DIR/${safe_name}_intents.json"

    echo "$INTENTS_JSON" | \
        jq ".intents[] | select(.protocol == \"$protocol\") | {
            id,
            protocol,
            src_chain,
            dst_chain,
            amount,
            timestamp,
            state,
            profit_usd,
            tx_hash
        }" > "$fixture_file"

    sample_count=$(jq -s 'length' "$fixture_file")
    echo "  ✓ $protocol: $sample_count samples → $fixture_file"
done

echo
echo "═══════════════════════════════════════════════════════════════════════"
echo "  FIXTURE EXTRACTION COMPLETE"
echo "═══════════════════════════════════════════════════════════════════════"
echo
echo "Summary:"
echo "  - Total intents: $INTENT_COUNT"
echo "  - Unique protocols: $PROTOCOL_COUNT"
echo "  - Fixtures directory: $FIXTURES_DIR"
echo "  - Full dump: $FULL_DUMP_FILE"
echo

# Extract unique chain IDs
echo "Extracting unique chain IDs from intents..."
CHAINS=$(echo "$INTENTS_JSON" | \
    jq -r '.intents[] | "\(.src_chain) \(.dst_chain)"' | \
    tr ' ' '\n' | sort -nu)

CHAIN_COUNT=$(echo "$CHAINS" | wc -l | tr -d ' ')

echo "Found $CHAIN_COUNT unique chains:"
echo "$CHAINS" | while read -r chain_id; do
    echo "  - Chain $chain_id"
done
echo

# Save chain list
CHAINS_FILE="$FIXTURES_DIR/chains-observed-$TIMESTAMP.txt"
echo "$CHAINS" > "$CHAINS_FILE"
echo "✓ Saved chain list to: $CHAINS_FILE"
echo

# Extract cross-chain routes
echo "Extracting cross-chain routes..."
ROUTES=$(echo "$INTENTS_JSON" | \
    jq -r '.intents[] | "\(.src_chain) → \(.dst_chain) via \(.protocol)"' | \
    sort -u)

ROUTE_COUNT=$(echo "$ROUTES" | wc -l | tr -d ' ')

echo "Found $ROUTE_COUNT unique routes:"
echo "$ROUTES" | head -20
if [ $ROUTE_COUNT -gt 20 ]; then
    echo "  ... and $((ROUTE_COUNT - 20)) more"
fi
echo

# Save routes
ROUTES_FILE="$FIXTURES_DIR/routes-observed-$TIMESTAMP.txt"
echo "$ROUTES" > "$ROUTES_FILE"
echo "✓ Saved routes to: $ROUTES_FILE"
echo

# Create summary JSON
SUMMARY_FILE="$FIXTURES_DIR/extraction-summary-$TIMESTAMP.json"
jq -n \
    --argjson intent_count "$INTENT_COUNT" \
    --argjson protocol_count "$PROTOCOL_COUNT" \
    --argjson chain_count "$CHAIN_COUNT" \
    --argjson route_count "$ROUTE_COUNT" \
    --arg timestamp "$TIMESTAMP" \
    '{
        timestamp: $timestamp,
        intent_count: $intent_count,
        protocol_count: $protocol_count,
        chain_count: $chain_count,
        route_count: $route_count,
        protocols: [],
        chains: [],
        routes: []
    }' > "$SUMMARY_FILE"

echo "✓ Saved summary to: $SUMMARY_FILE"
echo

echo "Fixture extraction complete! ✨"
