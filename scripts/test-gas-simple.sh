#!/bin/bash
set -e

WARMBED_API="http://46.4.96.124:30081"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🧪 GAS PRICE API SMOKE TEST"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo

# Test 1: Health check
echo -n "1. Warmbed API health... "
if curl -s -f -m 2 "$WARMBED_API/health" > /dev/null 2>&1; then
    echo "✅ OK"
else
    echo "❌ FAILED"
fi

# Test 2: All chains endpoint
echo -n "2. All chains gas data... "
ALL_GAS=$(curl -s -m 5 "$WARMBED_API/api/gas/latest" 2>&1)
if [ $? -eq 0 ]; then
    COUNT=$(echo "$ALL_GAS" | jq '. | length' 2>/dev/null || echo "0")
    echo "✅ OK ($COUNT chains)"
else
    echo "❌ FAILED"
    ALL_GAS="[]"
fi

# Test 3: Sample chains
echo
echo "3. Testing key chains:"
echo "   Chain | Name      | Status | Gas (gwei)"
echo "   ──────┼───────────┼────────┼───────────"

for chain_spec in "1:Ethereum" "10:Optimism" "8453:Base" "42161:Arbitrum" "137:Polygon"; do
    IFS=':' read -r chain_id chain_name <<< "$chain_spec"
    
    RESPONSE=$(curl -s -m 2 "$WARMBED_API/api/gas/latest/$chain_id" 2>&1)
    
    if [ $? -eq 0 ] && echo "$RESPONSE" | jq -e '.gas_price_gwei' > /dev/null 2>&1; then
        gas_price=$(echo "$RESPONSE" | jq -r '.gas_price_gwei')
        printf "   %-5s │ %-9s │ ✅ OK   │ %s\n" "$chain_id" "$chain_name" "$gas_price"
    else
        printf "   %-5s │ %-9s │ ❌ FAIL │ -\n" "$chain_id" "$chain_name"
    fi
done

# Test 4: Solver integration
echo
echo "4. Solver API integration:"
echo -n "   - Solver health... "
if curl -s -f -m 2 "http://localhost:8082/api/solver/stats" > /dev/null 2>&1; then
    echo "✅ OK"
    STATS=$(curl -s "http://localhost:8082/api/solver/stats" 2>&1)
    TOTAL=$(echo "$STATS" | jq -r '.total_intents // 0')
    echo "   - Total intents: $TOTAL"
else
    echo "❌ FAILED"
fi

echo
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "🏁 Smoke test complete"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
