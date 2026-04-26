#!/opt/homebrew/bin/bash
set -e

echo "═══════════════════════════════════════════════════════════════════════"
echo "  TAIFOON SOLVER COVERAGE MATRIX TEST"
echo "═══════════════════════════════════════════════════════════════════════"
echo

# ANSI color codes
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Chain definitions (all chains mentioned in protocols.xml + common chains)
declare -A CHAINS
CHAINS[1]="Ethereum"
CHAINS[10]="Optimism"
CHAINS[56]="BSC"
CHAINS[137]="Polygon"
CHAINS[8453]="Base"
CHAINS[42161]="Arbitrum"
CHAINS[200]="Solana"  # Mayan target
CHAINS[324]="zkSync Era"
CHAINS[534352]="Scroll"
CHAINS[59144]="Linea"
CHAINS[81457]="Blast"
CHAINS[7777777]="Zora"
CHAINS[34443]="Mode"
CHAINS[1135]="Lisk"

# Protocol list from protocols.xml
PROTOCOLS=(
  "across"
  "stargate"
  "lifi"
  "debridge"
  "mayan_swift"
  "wormhole"
  "hyperlane"
  "hop"
  "connext"
  "relay"
  "cctp"
  "arbitrum_bridge"
  "optimism_bridge"
  "socket"
  "squid"
  "rango"
  "layerzero"
  "t3rn"
  "celer"
  "synapse"
  "axelar"
  "1inch"
  "bancor"
  "uniswap_v2"
  "uniswap_v3"
  "uniswap_v4"
  "meson"
  "allbridge"
  "router_protocol"
  "symbiosis"
  "ccip"
)

# ═══════════════════════════════════════════════════════════════════════
# TEST 1: RAZOR GAS ENDPOINT COVERAGE
# ═══════════════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ TEST 1: RAZOR GAS ENDPOINT COVERAGE ═══${NC}"
echo

# Fetch razor data
RAZOR_JSON=$(curl -s "http://localhost:8082/api/solver/razor")
if [ $? -ne 0 ]; then
    echo -e "${RED}✗ Failed to fetch Razor endpoint${NC}"
    exit 1
fi

# Create summary arrays
declare -A RAZOR_STATUS

echo "Testing Razor gas data for all chains..."
echo

for chain_id in "${!CHAINS[@]}"; do
    chain_name="${CHAINS[$chain_id]}"

    # Check if chain exists in razor response
    READY=$(echo "$RAZOR_JSON" | jq -r ".presets[] | select(.chain_id == $chain_id) | .ready" 2>/dev/null)
    GAS_COST=$(echo "$RAZOR_JSON" | jq -r ".presets[] | select(.chain_id == $chain_id) | .gas_cost_gwei" 2>/dev/null)
    REASON=$(echo "$RAZOR_JSON" | jq -r ".presets[] | select(.chain_id == $chain_id) | .reason" 2>/dev/null)

    if [ -z "$READY" ]; then
        echo -e "  ${YELLOW}⊘${NC} Chain $chain_id ($chain_name): ${YELLOW}NOT IN RESPONSE${NC}"
        RAZOR_STATUS[$chain_id]="missing"
    elif [ "$READY" = "true" ] && [ "$GAS_COST" != "null" ] && [ "$GAS_COST" != "" ]; then
        # Validate gas cost is reasonable (0.001 to 10000 gwei)
        IS_REASONABLE=$(echo "$GAS_COST" | awk '{if ($1 > 0.001 && $1 < 10000) print "yes"; else print "no"}')
        if [ "$IS_REASONABLE" = "yes" ]; then
            echo -e "  ${GREEN}✓${NC} Chain $chain_id ($chain_name): ${GREEN}READY${NC} - Gas: ${GAS_COST} gwei"
            RAZOR_STATUS[$chain_id]="ready"
        else
            echo -e "  ${RED}✗${NC} Chain $chain_id ($chain_name): ${RED}UNREASONABLE GAS${NC} - Gas: ${GAS_COST} gwei"
            RAZOR_STATUS[$chain_id]="invalid_gas"
        fi
    else
        if [ -n "$REASON" ] && [ "$REASON" != "null" ]; then
            echo -e "  ${RED}✗${NC} Chain $chain_id ($chain_name): ${RED}NOT READY${NC} - ${REASON}"
        else
            echo -e "  ${RED}✗${NC} Chain $chain_id ($chain_name): ${RED}NOT READY${NC}"
        fi
        RAZOR_STATUS[$chain_id]="not_ready"
    fi
done

echo
echo -e "${BLUE}Razor Coverage Summary:${NC}"
READY_COUNT=0
NOT_READY_COUNT=0
MISSING_COUNT=0
INVALID_COUNT=0

for chain_id in "${!RAZOR_STATUS[@]}"; do
    status="${RAZOR_STATUS[$chain_id]}"
    case "$status" in
        ready)
            ((READY_COUNT++))
            ;;
        not_ready)
            ((NOT_READY_COUNT++))
            ;;
        missing)
            ((MISSING_COUNT++))
            ;;
        invalid_gas)
            ((INVALID_COUNT++))
            ;;
    esac
done

TOTAL_CHAINS=${#CHAINS[@]}
echo "  Ready: ${GREEN}$READY_COUNT${NC} / $TOTAL_CHAINS"
echo "  Not Ready: ${RED}$NOT_READY_COUNT${NC}"
echo "  Missing: ${YELLOW}$MISSING_COUNT${NC}"
echo "  Invalid Gas: ${RED}$INVALID_COUNT${NC}"
echo

# ═══════════════════════════════════════════════════════════════════════
# TEST 2: PROTOCOL INTENT DECODING STATUS
# ═══════════════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ TEST 2: PROTOCOL INTENT DECODING STATUS ═══${NC}"
echo

# Fetch current intents
INTENTS_JSON=$(curl -s "http://localhost:8082/api/solver/intents")
INTENT_COUNT=$(echo "$INTENTS_JSON" | jq '.intents | length')

echo "Current intent count: $INTENT_COUNT"
echo

if [ "$INTENT_COUNT" -gt 0 ]; then
    echo "Sample intents (first 3):"
    echo "$INTENTS_JSON" | jq '.intents[0:3]' | head -50
    echo

    # Analyze protocol distribution
    echo -e "${BLUE}Protocol Distribution:${NC}"
    echo "$INTENTS_JSON" | jq -r '.intents[].protocol' | sort | uniq -c | sort -rn | head -10
    echo

    # Check for decoding errors (state should not be "error")
    ERROR_COUNT=$(echo "$INTENTS_JSON" | jq '[.intents[] | select(.state == "error")] | length')
    if [ "$ERROR_COUNT" -gt 0 ]; then
        echo -e "${RED}✗ Found $ERROR_COUNT intents with decoding errors${NC}"
        echo "$INTENTS_JSON" | jq '.intents[] | select(.state == "error")' | head -20
    else
        echo -e "${GREEN}✓ No decoding errors found${NC}"
    fi
else
    echo -e "${YELLOW}⊘ No intents yet - this is expected for a fresh instance${NC}"
    echo "  Connect to Genome SSE at http://46.4.96.124:30081/api/genome/subscribe/sse to receive intents"
fi
echo

# ═══════════════════════════════════════════════════════════════════════
# TEST 3: PROTOCOLS ENDPOINT
# ═══════════════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ TEST 3: PROTOCOLS ENDPOINT ═══${NC}"
echo

PROTOCOLS_JSON=$(curl -s "http://localhost:8082/api/solver/protocols")
PROTOCOL_COUNT=$(echo "$PROTOCOLS_JSON" | jq '.protocols | length')

echo "Active protocols: $PROTOCOL_COUNT"
if [ "$PROTOCOL_COUNT" -gt 0 ]; then
    echo "$PROTOCOLS_JSON" | jq '.protocols[] | {name, fills, profit_usd, volume_usd}'
else
    echo -e "${YELLOW}⊘ No protocol data yet${NC}"
fi
echo

# ═══════════════════════════════════════════════════════════════════════
# TEST 4: STATS ENDPOINT VALIDATION
# ═══════════════════════════════════════════════════════════════════════
echo -e "${BLUE}═══ TEST 4: STATS ENDPOINT VALIDATION ═══${NC}"
echo

STATS_JSON=$(curl -s "http://localhost:8082/api/solver/stats")
echo "$STATS_JSON" | jq '.'

# Check for hardcoded values
LATENCY=$(echo "$STATS_JSON" | jq -r '.latency_ms')
SUCCESS_RATE=$(echo "$STATS_JSON" | jq -r '.success_rate')

if [ "$LATENCY" = "127" ] && [ "$SUCCESS_RATE" = "0.942" ]; then
    echo -e "${YELLOW}⚠ Warning: Stats contain hardcoded initial values (latency=127, success_rate=0.942)${NC}"
    echo "  This is OK for a fresh instance but should update with real data"
fi
echo

# ═══════════════════════════════════════════════════════════════════════
# FINAL SUMMARY
# ═══════════════════════════════════════════════════════════════════════
echo "═══════════════════════════════════════════════════════════════════════"
echo -e "${BLUE}  AUTONOMOUS DELIVERY READINESS MATRIX${NC}"
echo "═══════════════════════════════════════════════════════════════════════"
echo

echo "RAZOR GAS COVERAGE (by chain):"
echo "------------------------------"
for chain_id in $(echo "${!CHAINS[@]}" | tr ' ' '\n' | sort -n); do
    chain_name="${CHAINS[$chain_id]}"
    status="${RAZOR_STATUS[$chain_id]}"

    case "$status" in
        ready)
            printf "  [${GREEN}✓${NC}] %-4s %-20s READY\n" "$chain_id" "$chain_name"
            ;;
        not_ready)
            printf "  [${RED}✗${NC}] %-4s %-20s NOT READY\n" "$chain_id" "$chain_name"
            ;;
        missing)
            printf "  [${YELLOW}⊘${NC}] %-4s %-20s MISSING\n" "$chain_id" "$chain_name"
            ;;
        invalid_gas)
            printf "  [${RED}✗${NC}] %-4s %-20s INVALID GAS\n" "$chain_id" "$chain_name"
            ;;
    esac
done
echo

echo "PROTOCOL SUPPORT (from protocols.xml):"
echo "--------------------------------------"
echo "Total protocols defined: ${#PROTOCOLS[@]}"
echo
echo "Cross-chain bridges:"
for proto in across stargate debridge mayan_swift wormhole hop connext relay cctp celer synapse meson allbridge router_protocol symbiosis; do
    echo "  - $proto"
done
echo
echo "Messaging layers:"
for proto in hyperlane layerzero axelar ccip; do
    echo "  - $proto"
done
echo
echo "Native bridges:"
for proto in arbitrum_bridge optimism_bridge; do
    echo "  - $proto"
done
echo
echo "Aggregators:"
for proto in lifi socket squid rango; do
    echo "  - $proto"
done
echo
echo "DEX (for reference, not cross-chain):"
for proto in uniswap_v2 uniswap_v3 uniswap_v4 1inch bancor; do
    echo "  - $proto"
done
echo
echo "t3rn Lambda:"
echo "  - t3rn (autonomous delivery target)"
echo

echo "═══════════════════════════════════════════════════════════════════════"
echo -e "${GREEN}  COVERAGE TEST COMPLETE${NC}"
echo "═══════════════════════════════════════════════════════════════════════"
