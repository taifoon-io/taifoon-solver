#!/usr/bin/env python3
"""
Create a tiny deBridge DLN self-fill order using USDC as the give/take token.

Auto-selects the source chain with the most ETH (to pay the 0.001 ETH fixed fee)
and directs the fill to the destination chain with the most USDC (solver fills it).
The solver's own address is set as allowedTakerDst — only our solver can fill it.

Requirements on source chain:
  - ETH balance >= 0.001 ETH (fixed protocol fee) + gas (~0.0001 ETH)
  - USDC balance >= GIVE_AMOUNT_USDC + operating expenses (~$0.29 on a $0.50 order)
  - Must approve USDC to DLN_SOURCE address first (see --approve flag)

Path to fund ETH on Base (currently 0.000142 ETH):
    python3 tools/swap_usdt_to_eth.py --dry-run   # preview: swap $3 USDT → ~0.001274 ETH
    python3 tools/swap_usdt_to_eth.py --approve   # approve USDT to Uniswap SwapRouter
    python3 tools/swap_usdt_to_eth.py             # execute swap (gives ~0.001415 ETH on Base)
    python3 tools/create_debridge_usdc_self_fill.py --src 8453 --approve
    python3 tools/create_debridge_usdc_self_fill.py --src 8453

Usage:
    python3 tools/create_debridge_usdc_self_fill.py --dry-run        # preview best chain
    python3 tools/create_debridge_usdc_self_fill.py --approve        # approve USDC on best src
    python3 tools/create_debridge_usdc_self_fill.py                  # broadcast order
    python3 tools/create_debridge_usdc_self_fill.py --src 8453       # force Base as source

Environment:
    SOLVER_PRIVATE_KEY   EVM private key (or loaded from macOS keychain 'mamba-messiah-key')
"""

import subprocess
import os
import sys
import json
import time
import urllib.request

FIXED_FEE_WEI = 1_000_000_000_000_000   # 0.001 ETH fixed protocol fee
GIVE_AMOUNT_RAW = 500_000                # 0.50 USDC (6 decimals) — deBridge API minimum; take ≈ $0.2154 USDC
DLN_SOURCE = "0xeF4fB24aD0916217251F553c0596F8Edc630EB66"   # same on all chains

# Chain configs: chain_id → (name, rpc, usdc_addr, has_usdc_for_fill)
CHAINS = {
    8453: {
        "name": "Base",
        "rpc": "https://base-rpc.publicnode.com",
        "usdc": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
    },
    10: {
        "name": "Optimism",
        "rpc": "https://mainnet.optimism.io",
        "usdc": "0x0b2C639c533813f4Aa9D7837CAf62653d097Ff85",
    },
    42161: {
        "name": "Arbitrum",
        "rpc": "https://arb1.arbitrum.io/rpc",
        "usdc": "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
    },
}


def get_solver_address(private_key: str) -> str:
    r = subprocess.run(['cast', 'wallet', 'address', private_key],
                       capture_output=True, text=True)
    return r.stdout.strip()


def get_eth_balance(solver: str, rpc: str) -> int:
    r = subprocess.run(['cast', 'balance', solver, '--rpc-url', rpc],
                       capture_output=True, text=True)
    return int(r.stdout.strip() or '0')


def parse_cast_uint(s: str) -> int:
    """Parse cast call output which may be '971835 [9.718e5]' or just '971835'."""
    s = s.strip().split('\n')[0].split('[')[0].strip()
    return int(s or '0')


def get_usdc_balance(solver: str, usdc: str, rpc: str) -> int:
    r = subprocess.run(['cast', 'call', usdc, 'balanceOf(address)(uint256)', solver,
                        '--rpc-url', rpc], capture_output=True, text=True)
    return parse_cast_uint(r.stdout)


def get_usdc_allowance(solver: str, usdc: str, spender: str, rpc: str) -> int:
    r = subprocess.run(['cast', 'call', usdc,
                        'allowance(address,address)(uint256)', solver, spender,
                        '--rpc-url', rpc], capture_output=True, text=True)
    return parse_cast_uint(r.stdout)


def get_take_amount_from_api(src_chain: int, src_usdc: str, dst_chain: int, dst_usdc: str,
                              solver: str) -> int:
    url = (f"https://api.dln.trade/v1.0/dln/order/create-tx"
           f"?srcChainId={src_chain}"
           f"&srcChainTokenIn={src_usdc}"
           f"&srcChainTokenInAmount={GIVE_AMOUNT_RAW}"
           f"&dstChainId={dst_chain}"
           f"&dstChainTokenOut={dst_usdc}"
           f"&dstChainTokenOutRecipient={solver}"
           f"&prependOperatingExpenses=false")
    fallback = 100_000
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "taifoon-1.0"})
        with urllib.request.urlopen(req, timeout=8) as r:
            data = json.loads(r.read())
        dst_info = data.get('estimation', {}).get('dstChainTokenOut', {})
        rec_amt = dst_info.get('recommendedAmount', str(fallback))
        print(f"  deBridge API estimated take amount: {rec_amt} raw ({int(rec_amt)/1e6:.4f} USDC)")
        return int(rec_amt)
    except Exception as e:
        print(f"  deBridge API error: {e}, using default {fallback/1e6:.4f} USDC")
        return fallback


def abi_encode_bytes(data: bytes) -> bytes:
    length = len(data)
    padded_len = (length + 31) // 32 * 32
    return length.to_bytes(32, 'big') + data + b'\x00' * (padded_len - length)


def build_create_salted_order_calldata(solver_addr: str, src_usdc: str, dst_usdc: str,
                                        dst_chain: int, take_amount_raw: int) -> str:
    def strip0x(s): return s[2:] if s.startswith('0x') or s.startswith('0X') else s
    solver_bytes = bytes.fromhex(strip0x(solver_addr))
    src_usdc_bytes = bytes.fromhex(strip0x(src_usdc))
    dst_usdc_bytes = bytes.fromhex(strip0x(dst_usdc))

    f_give_token = src_usdc_bytes.rjust(32, b'\x00')
    f_give_amount = GIVE_AMOUNT_RAW.to_bytes(32, 'big')
    f_take_amount = take_amount_raw.to_bytes(32, 'big')
    f_take_chain = dst_chain.to_bytes(32, 'big')
    f_patch_auth = solver_bytes.rjust(32, b'\x00')

    dyn_take_token = dst_usdc_bytes
    dyn_receiver = solver_bytes
    dyn_order_auth = solver_bytes
    dyn_allowed_taker = solver_bytes
    dyn_external_call = b''
    dyn_cancel_beneficiary = b''

    head_count = 11
    base_offset = head_count * 32

    def dyn_size(data): return 32 + ((len(data) + 31) // 32) * 32

    offsets = []
    current = 0
    for data in [dyn_take_token, dyn_receiver, dyn_order_auth, dyn_allowed_taker,
                 dyn_external_call, dyn_cancel_beneficiary]:
        offsets.append((base_offset + current).to_bytes(32, 'big'))
        current += dyn_size(data)

    head = (f_give_token + f_give_amount + offsets[0] + f_take_amount + f_take_chain
            + offsets[1] + f_patch_auth + offsets[2] + offsets[3] + offsets[4] + offsets[5])

    tail = b''
    for data in [dyn_take_token, dyn_receiver, dyn_order_auth, dyn_allowed_taker,
                 dyn_external_call, dyn_cancel_beneficiary]:
        tail += abi_encode_bytes(data)

    struct_encoded = head + tail
    salt = int(time.time()).to_bytes(32, 'big')
    top_base = 5 * 32

    def outer_dyn_size(d): return 32 + ((len(d) + 31) // 32) * 32

    struct_off = top_base.to_bytes(32, 'big')
    curr = outer_dyn_size(struct_encoded)
    affiliate_off = (top_base + curr).to_bytes(32, 'big')
    curr += 64
    permit_off = (top_base + curr).to_bytes(32, 'big')
    curr += 64
    meta_off = (top_base + curr).to_bytes(32, 'big')

    top_head = struct_off + salt + affiliate_off + permit_off + meta_off
    top_tail = (abi_encode_bytes(struct_encoded) + abi_encode_bytes(b'')
                + abi_encode_bytes(b'') + abi_encode_bytes(b''))

    selector = bytes.fromhex('b9303701')
    return '0x' + (selector + top_head + top_tail).hex()


def main():
    dry_run = '--dry-run' in sys.argv
    do_approve = '--approve' in sys.argv

    forced_src = None
    for i, arg in enumerate(sys.argv):
        if arg == '--src' and i + 1 < len(sys.argv):
            forced_src = int(sys.argv[i + 1])

    pk = os.environ.get('SOLVER_PRIVATE_KEY', '')
    if not pk:
        r = subprocess.run(['security', 'find-generic-password', '-s', 'mamba-messiah-key', '-w'],
                           capture_output=True, text=True)
        pk = r.stdout.strip()
    if not pk:
        print("ERROR: SOLVER_PRIVATE_KEY not set", file=sys.stderr)
        sys.exit(1)

    solver_addr = get_solver_address(pk)
    print(f"Solver address: {solver_addr}")

    # Scan all chains for ETH and USDC balances
    print("\nScanning chain balances...")
    balances = {}
    for chain_id, cfg in CHAINS.items():
        eth = get_eth_balance(solver_addr, cfg['rpc'])
        usdc = get_usdc_balance(solver_addr, cfg['usdc'], cfg['rpc'])
        balances[chain_id] = (eth, usdc)
        eth_f = eth / 1e18
        usdc_f = usdc / 1e6
        pct = eth / FIXED_FEE_WEI * 100
        short = max(0, (FIXED_FEE_WEI - eth) / 1e18)
        flag = "✅" if eth >= FIXED_FEE_WEI else f"⚠️  (need +{short:.6f} ETH, {pct:.1f}% ready)"
        print(f"  {cfg['name']:10} chain={chain_id}: {eth_f:.6f} ETH  ${usdc_f:.4f} USDC {flag}")

    # Pick best source: forced, or chain with most ETH and >= GIVE_AMOUNT_RAW USDC
    if forced_src:
        src_chain = forced_src
    else:
        candidates = [(cid, eth, usdc) for cid, (eth, usdc) in balances.items()
                      if usdc >= GIVE_AMOUNT_RAW]
        if not candidates:
            # Fall back to chain with most ETH even if USDC is insufficient
            candidates = list(balances.items())
            candidates = [(cid, eth, usdc) for cid, (eth, usdc) in candidates]
        src_chain = max(candidates, key=lambda x: x[1])[0]

    src_cfg = CHAINS[src_chain]
    src_eth, src_usdc = balances[src_chain]

    # Pick destination: chain with most USDC (excluding source) — this is where solver fills
    dst_candidates = [(cid, usdc) for cid, (eth, usdc) in balances.items() if cid != src_chain]
    dst_chain = max(dst_candidates, key=lambda x: x[1])[0]
    dst_cfg = CHAINS[dst_chain]
    dst_usdc_bal = balances[dst_chain][1]

    print(f"\nSelected route: {src_cfg['name']} (chain={src_chain}) → {dst_cfg['name']} (chain={dst_chain})")
    print(f"  Source: {src_eth/1e18:.6f} ETH, ${src_usdc/1e6:.4f} USDC")
    print(f"  Dest:   ${dst_usdc_bal/1e6:.4f} USDC (solver fills from this balance)")

    if src_eth < FIXED_FEE_WEI:
        shortfall = (FIXED_FEE_WEI - src_eth) / 1e18
        print(f"\n⚠️  BLOCKED on {src_cfg['name']}: need +{shortfall:.6f} ETH for fixed protocol fee.")
        print(f"  ({src_eth/1e18:.6f} / {FIXED_FEE_WEI/1e18:.4f} ETH = {src_eth/FIXED_FEE_WEI*100:.1f}%)")
        if not dry_run:
            print("Re-run with --dry-run to preview calldata anyway.")
            sys.exit(1)

    if src_usdc < GIVE_AMOUNT_RAW:
        shortfall_usd = (GIVE_AMOUNT_RAW - src_usdc) / 1e6
        print(f"\n⚠️  BLOCKED on {src_cfg['name']}: need +${shortfall_usd:.4f} USDC give amount.")
        if not dry_run:
            sys.exit(1)

    if do_approve:
        # Step A: approve DlnSource on src chain (give side — needed to call createSaltedOrder)
        allowance = get_usdc_allowance(solver_addr, src_cfg['usdc'], DLN_SOURCE, src_cfg['rpc'])
        print(f"\n[A] DlnSource allowance on {src_cfg['name']}: ${allowance/1e6:.4f}")
        if allowance >= GIVE_AMOUNT_RAW:
            print("  ✅ Already approved for DlnSource.")
        else:
            print(f"  Approving ${GIVE_AMOUNT_RAW/1e6:.4f} USDC to DLN_SOURCE on {src_cfg['name']}...")
            result = subprocess.run([
                'cast', 'send', src_cfg['usdc'],
                'approve(address,uint256)', DLN_SOURCE, str(GIVE_AMOUNT_RAW),
                '--rpc-url', src_cfg['rpc'], '--private-key', pk,
            ], capture_output=True, text=True)
            if result.returncode != 0:
                print("❌ DlnSource approve failed:", result.stderr[:300])
                sys.exit(1)
            print("  ✅ DlnSource approved")

        # Step B: approve DlnDestination on dst chain (take side — needed to call fulfillOrder)
        DLN_DEST = "0xE7351Fd770A37282b91D153Ee690B63579D6dd7f"
        dst_allow = get_usdc_allowance(solver_addr, dst_cfg['usdc'], DLN_DEST, dst_cfg['rpc'])
        print(f"\n[B] DlnDestination allowance on {dst_cfg['name']}: ${dst_allow/1e6:.4f}")
        MAX_UINT = 2**256 - 1
        if dst_allow > 10**30:
            print("  ✅ Already max-approved for DlnDestination.")
        else:
            print(f"  Approving max USDC to DLN_DEST on {dst_cfg['name']}...")
            result = subprocess.run([
                'cast', 'send', dst_cfg['usdc'],
                'approve(address,uint256)', DLN_DEST, str(MAX_UINT),
                '--rpc-url', dst_cfg['rpc'], '--private-key', pk,
            ], capture_output=True, text=True)
            if result.returncode != 0:
                print("❌ DlnDestination approve failed:", result.stderr[:300])
                sys.exit(1)
            print("  ✅ DlnDestination approved")
        return

    print(f"\nFetching take amount from deBridge API ({src_cfg['name']} → {dst_cfg['name']})...")
    take_amount = get_take_amount_from_api(src_chain, src_cfg['usdc'], dst_chain,
                                           dst_cfg['usdc'], solver_addr)
    if take_amount <= 0:
        print("❌ Invalid take amount from API")
        sys.exit(1)

    print(f"\nOrder parameters:")
    print(f"  Give: ${GIVE_AMOUNT_RAW/1e6:.4f} USDC on {src_cfg['name']}")
    print(f"  Take: ${take_amount/1e6:.4f} USDC on {dst_cfg['name']}")
    print(f"  Allowed taker: {solver_addr} (ONLY our solver)")
    print(f"  Protocol fee: {FIXED_FEE_WEI/1e18:.4f} ETH")

    calldata = build_create_salted_order_calldata(
        solver_addr, src_cfg['usdc'], dst_cfg['usdc'], dst_chain, take_amount)
    print(f"\nCalldata: {calldata[:60]}...")

    if dry_run:
        print(f"\n[DRY RUN] Simulating on {src_cfg['name']}...")
        sim = subprocess.run([
            'cast', 'call', DLN_SOURCE, '--rpc-url', src_cfg['rpc'],
            '--from', solver_addr, '--value', str(FIXED_FEE_WEI), calldata,
        ], capture_output=True, text=True)
        print("stdout:", sim.stdout[:400])
        print("stderr:", sim.stderr[:400])
        print("\n[DRY RUN] Done. Remove --dry-run to broadcast.")
        return

    allowance = get_usdc_allowance(solver_addr, src_cfg['usdc'], DLN_SOURCE, src_cfg['rpc'])
    if allowance < GIVE_AMOUNT_RAW:
        print(f"\n⚠️  Insufficient DlnSource allowance (${allowance/1e6:.4f}). Run with --approve first.")
        sys.exit(1)
    DLN_DEST = "0xE7351Fd770A37282b91D153Ee690B63579D6dd7f"
    dst_allow = get_usdc_allowance(solver_addr, dst_cfg['usdc'], DLN_DEST, dst_cfg['rpc'])
    if dst_allow <= 10**30 // 10**6:  # less than 10^24 raw (way below max-approved)
        print(f"\n⚠️  DlnDestination allowance on {dst_cfg['name']} is ${dst_allow/1e6:.4f}.")
        print(f"   Run with --approve to approve DlnDestination before broadcasting the order.")
        sys.exit(1)

    print(f"\nBroadcasting createSaltedOrder on {src_cfg['name']}...")
    result = subprocess.run([
        'cast', 'send', DLN_SOURCE, '--rpc-url', src_cfg['rpc'],
        '--private-key', pk, '--value', str(FIXED_FEE_WEI), '--gas-limit', '400000',
        calldata,
    ], capture_output=True, text=True)
    print("stdout:", result.stdout)
    print("stderr:", result.stderr)

    if result.returncode == 0:
        print(f"\n✅ Order created on {src_cfg['name']}! Solver should detect and fill it within ~12s.")
        print(f"   Monitor with: PROTOCOL_FILTER=debridge DRY_RUN=false ./run-mainnet.sh")
    else:
        print("\n❌ Failed to create order")
        sys.exit(1)


if __name__ == '__main__':
    main()
