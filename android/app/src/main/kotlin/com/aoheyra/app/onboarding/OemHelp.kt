package com.aoheyra.app.onboarding

import com.aoheyra.app.R

/**
 * Vendor keys used by the AND-20 results table. Unknown devices fall back to
 * [OTHER] so the screen never has to guess a vendor from an unverified value.
 */
enum class OemVendor(val id: String, val labelRes: Int) {
    PIXEL("pixel", R.string.oem_vendor_pixel),
    SAMSUNG("samsung", R.string.oem_vendor_samsung),
    XIAOMI("xiaomi", R.string.oem_vendor_xiaomi),
    ONEPLUS("oneplus", R.string.oem_vendor_oneplus),
    OTHER("other", R.string.oem_vendor_other);

    companion object {
        fun from(id: String?): OemVendor = OemVendor.values().firstOrNull { it.id == id } ?: OTHER
    }
}

/** One onboarding step. Text lives in resources so it can be translated. */
data class OemHelpStep(val titleRes: Int, val detailRes: Int)

object OemHelp {
    /** Shown for every vendor before any vendor-specific instruction. */
    val commonSteps: List<OemHelpStep> = listOf(
        OemHelpStep(
            R.string.oem_common_permissions_title,
            R.string.oem_common_permissions_detail,
        ),
        OemHelpStep(
            R.string.oem_common_battery_title,
            R.string.oem_common_battery_detail,
        ),
    )

    /**
     * Returns the steps for [vendor]. The text says "check" rather than
     * asserting that a setting exists: the AND-20 matrix records which steps
     * were actually verified on which device.
     */
    fun stepsFor(vendor: OemVendor): List<OemHelpStep> = commonSteps + when (vendor) {
        OemVendor.PIXEL -> listOf(
            OemHelpStep(R.string.oem_pixel_title, R.string.oem_pixel_detail),
        )
        OemVendor.SAMSUNG -> listOf(
            OemHelpStep(R.string.oem_samsung_title, R.string.oem_samsung_detail),
        )
        OemVendor.XIAOMI -> listOf(
            OemHelpStep(R.string.oem_xiaomi_title, R.string.oem_xiaomi_detail),
        )
        OemVendor.ONEPLUS -> listOf(
            OemHelpStep(R.string.oem_oneplus_title, R.string.oem_oneplus_detail),
        )
        OemVendor.OTHER -> listOf(
            OemHelpStep(R.string.oem_other_title, R.string.oem_other_detail),
        )
    }
}
