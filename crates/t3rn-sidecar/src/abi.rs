use alloy::sol;

sol! {
    #[sol(rpc)]
    contract LiquidityWellCompact {
        function order(
            bytes4 destination,
            uint32 asset,
            bytes32 targetAccount,
            uint256 amount,
            address rewardAsset,
            uint256 insurance,
            uint256 maxReward
        ) external payable;

        event OrderCreated(
            bytes32 indexed orderId,
            address indexed user,
            bytes4 destination,
            uint256 amount
        );
    }
}
