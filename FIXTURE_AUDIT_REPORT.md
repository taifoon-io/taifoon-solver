====================================================================================================
COMPREHENSIVE FIXTURE AUDIT REPORT
====================================================================================================

SUMMARY
----------------------------------------------------------------------------------------------------
Total Intents: 42
Valid Intents: 6 (14.3%)
Intents with Issues: 36 (85.7%)

PROTOCOL BREAKDOWN
----------------------------------------------------------------------------------------------------

ACROSS_V3:
  Total: 5
  Valid: 0 (0.0%)
  Issues:
    - negative_profit: 5

LIFI_V2:
  Total: 17
  Valid: 1 (5.9%)
  Issues:
    - negative_profit: 16

ORBITER_FINANCE:
  Total: 16
  Valid: 4 (25.0%)
  Issues:
    - negative_profit: 11
    - unrealistic_profit: 1

STARGATE_V2:
  Total: 4
  Valid: 1 (25.0%)
  Issues:
    - negative_profit: 3


DETAILED ISSUES BY PROTOCOL
====================================================================================================

ACROSS_V3 - 5 issues
----------------------------------------------------------------------------------------------------

MEDIUM (5 issues):

  [NEGATIVE_PROFIT] Negative profit $-1.04 (likely gas calculation bug)
    ID: across_v3:across_v3::across_2251139
    profit: -1.0403
    amount: 19900000000000000
    src_chain: 59144 (Linea)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.04 (likely gas calculation bug)
    ID: across_v3:across_v3::across_2251139
    profit: -1.0403
    amount: 19900000000000000
    src_chain: 59144 (Linea)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.09 (likely gas calculation bug)
    ID: across_v3:across_v3::across_3661683
    profit: -0.09288232590460253
    amount: 2372558031799159
    src_chain: 10 (Optimism)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.09 (likely gas calculation bug)
    ID: across_v3:across_v3::across_3661683
    profit: -0.09288232590460253
    amount: 2372558031799159
    src_chain: 10 (Optimism)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-5.09 (likely gas calculation bug)
    ID: across_v3:across_v3::across_4311877
    profit: -5.087075742156303
    amount: 4308085947898706
    src_chain: 42161 (Arbitrum)
    dst_chain: 1 (Ethereum)
    state: skipped


LIFI_V2 - 16 issues
----------------------------------------------------------------------------------------------------

MEDIUM (16 issues):

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x8f402c380754a14c8a216c67e219af96c8a449b6b6cd08553d455945e616bba4
    profit: -1.0499999999933585
    amount: 2213850
    src_chain: 59144 (Linea)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x8f402c380754a14c8a216c67e219af96c8a449b6b6cd08553d455945e616bba4
    profit: -1.0499999999933585
    amount: 2213850
    src_chain: 59144 (Linea)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xca7dfe16209a32914686f5c49d948d97fcd41ee117cf78777a68e1a049700622
    profit: -1.99999999997
    amount: 10000000
    src_chain: 999 (?)
    dst_chain: 56 (BSC)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xa80790f7a1e17fa49eb5bdc64a13bd56e2605d4ec4863704a24ff692ce3a236f
    profit: -1.9999999999928413
    amount: 2386268
    src_chain: 34443 (?)
    dst_chain: 56 (BSC)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xa80790f7a1e17fa49eb5bdc64a13bd56e2605d4ec4863704a24ff692ce3a236f
    profit: -1.9999999999928413
    amount: 2386268
    src_chain: 34443 (?)
    dst_chain: 56 (BSC)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x23ba9f31e5214dd9463c5d132b5fe7d34f84672d81aad015e31f052220d1c137
    profit: -1.0496018613113902
    amount: 132712896203241
    src_chain: 324 (?)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x23ba9f31e5214dd9463c5d132b5fe7d34f84672d81aad015e31f052220d1c137
    profit: -1.0496018613113902
    amount: 132712896203241
    src_chain: 324 (?)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.09 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x4baa01da3057c307dfc081be7dd943318af5704410994e59d1493d29b662bb4f
    profit: -0.09288232590460253
    amount: 2372558031799159
    src_chain: 10 (Optimism)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.09 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x4baa01da3057c307dfc081be7dd943318af5704410994e59d1493d29b662bb4f
    profit: -0.09288232590460253
    amount: 2372558031799159
    src_chain: 10 (Optimism)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.10 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xef4e8a6a1f9f2dab703a4bf928b2e7ff49675191471d8f5a05f4a74802b73e15
    profit: -1.0980894697398462
    amount: 636843420051273
    src_chain: 59144 (Linea)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.10 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xef4e8a6a1f9f2dab703a4bf928b2e7ff49675191471d8f5a05f4a74802b73e15
    profit: -1.0980894697398462
    amount: 636843420051273
    src_chain: 59144 (Linea)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x92c34428344cbea88e0e7045b390ee2c39959f5c936e17f25e34e2c451d72148
    profit: -1.0471601576149265
    amount: 946614128357811
    src_chain: 59144 (Linea)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0x92c34428344cbea88e0e7045b390ee2c39959f5c936e17f25e34e2c451d72148
    profit: -1.0471601576149265
    amount: 946614128357811
    src_chain: 59144 (Linea)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xe05ae61c4c8b17babaa4ff3b4ca1fcfe0e8290f9ad2ec83de1a8391550f3798c
    profit: -1.0499999999562568
    amount: 14581064
    src_chain: 43114 (Avalanche)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-6.00 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xb7224a35c61cac2b8ac6cbdab06a0bd747f98f331ba75e7815de2f27cedf9ab5
    profit: -5.999999849544769
    amount: 50151743826
    src_chain: 999 (?)
    dst_chain: 1 (Ethereum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-6.00 (likely gas calculation bug)
    ID: lifi_v2:lifi_v2::lifi_0xb7224a35c61cac2b8ac6cbdab06a0bd747f98f331ba75e7815de2f27cedf9ab5
    profit: -5.999999849544769
    amount: 50151743826
    src_chain: 999 (?)
    dst_chain: 1 (Ethereum)
    state: skipped


ORBITER_FINANCE - 12 issues
----------------------------------------------------------------------------------------------------

MEDIUM (11 issues):

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x078343b7fecc3d74516d26a56236101eb83ca7ef_59144
    profit: -1.99999999861616
    amount: 461280000
    src_chain: 143 (Monad)
    dst_chain: 59144 (Linea)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x078343b7fecc3d74516d26a56236101eb83ca7ef_59144
    profit: -1.99999999861616
    amount: 461280000
    src_chain: 143 (Monad)
    dst_chain: 59144 (Linea)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.05 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x2e32345bf0592bff19313831b99900c530d37d90_8453
    profit: -1.0499999897601
    amount: 3413300000
    src_chain: 143 (Monad)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-6.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x70bd7cb09fac471987a593325f27a913e11fe21b_34443
    profit: -5.99649517737006
    amount: 1168274209980000
    src_chain: 1 (Ethereum)
    dst_chain: 34443 (?)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-6.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x70bd7cb09fac471987a593325f27a913e11fe21b_34443
    profit: -5.99649517737006
    amount: 1168274209980000
    src_chain: 1 (Ethereum)
    dst_chain: 34443 (?)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0xddef43f9572d9a62fd26aa3e2ada21927e32dbeb_1284
    profit: -1.99999999997192
    amount: 9360000
    src_chain: 43114 (Avalanche)
    dst_chain: 1284 (Moonbeam)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0xfd7ce1473a2dac8bb31cb1bbf2f5d7dd613912b1_43114
    profit: -1.99999999996982
    amount: 10060000
    src_chain: 999 (?)
    dst_chain: 43114 (Avalanche)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-2.00 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0xfd7ce1473a2dac8bb31cb1bbf2f5d7dd613912b1_43114
    profit: -1.99999999996982
    amount: 10060000
    src_chain: 999 (?)
    dst_chain: 43114 (Avalanche)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.46 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x3bdb03ad7363152dfbc185ee23ebc93f0cf93fd1_42161
    profit: -0.45524231048102004
    amount: 214919229839660000
    src_chain: 324 (?)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.46 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0xb4ab2ff34fadc774aff45f1c4566cb5e16bd4867_42161
    profit: -0.45859836415694
    amount: 213800545281020000
    src_chain: 324 (?)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-0.39 (likely gas calculation bug)
    ID: orbiter_finance:orbiter_finance::0x3bdb03ad7363152dfbc185ee23ebc93f0cf93fd1_8453
    profit: -0.39337420758489006
    amount: 218875264138370000
    src_chain: 324 (?)
    dst_chain: 8453 (Base)
    state: skipped


LOW (1 issues):

  [UNREALISTIC_PROFIT] Profit margin 25418881.8% seems unrealistic (>$3.37 on ~$0.00)
    ID: orbiter_finance:orbiter_finance::0x17adbb47735c68b75e405abceaba52a7f6522e17_250
    profit: 3.3722900000000005
    estimated_amount_usd: 1.326687e-05
    profit_margin_pct: 25418881.77


STARGATE_V2 - 3 issues
----------------------------------------------------------------------------------------------------

MEDIUM (3 issues):

  [NEGATIVE_PROFIT] Negative profit $-0.10 (likely gas calculation bug)
    ID: stargate_v2:stargate_v2::0x0000000000000000000000000000000000000000000000000000000000000000
    profit: -0.0999999985
    amount: 500000000
    src_chain: 10 (Optimism)
    dst_chain: 8453 (Base)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.10 (likely gas calculation bug)
    ID: stargate_v2:stargate_v2::0x6b609ac3a47e9ff2f07c629b84b292c322b189b5cf34aa52a910fc5e0cc58081
    profit: -1.09999808
    amount: 640000000000
    src_chain: 143 (Monad)
    dst_chain: 42161 (Arbitrum)
    state: skipped

  [NEGATIVE_PROFIT] Negative profit $-1.10 (likely gas calculation bug)
    ID: stargate_v2:stargate_v2::0x6b609ac3a47e9ff2f07c629b84b292c322b189b5cf34aa52a910fc5e0cc58081
    profit: -1.09999808
    amount: 640000000000
    src_chain: 143 (Monad)
    dst_chain: 42161 (Arbitrum)
    state: skipped


====================================================================================================
END OF REPORT
====================================================================================================