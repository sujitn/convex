package com.convex;

/** Spread family. Wire values match {@code convex_core::types::SpreadType}. */
public enum SpreadType {
    Z_SPREAD("ZSpread"),
    G_SPREAD("GSpread"),
    I_SPREAD("ISpread"),
    ASSET_SWAP_PAR("AssetSwapPar"),
    ASSET_SWAP_PROCEEDS("AssetSwapProceeds"),
    OAS("OAS"),
    CREDIT("Credit"),
    DISCOUNT_MARGIN("DiscountMargin");

    private final String wire;

    SpreadType(String wire) {
        this.wire = wire;
    }

    public String wire() {
        return wire;
    }
}
