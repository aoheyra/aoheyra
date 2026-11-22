package com.aoheyra.app.onboarding

import org.junit.Assert.assertEquals
import org.junit.Test

/** AND-20: help steps must be complete for every vendor key. */
class OemHelpTest {
    @Test
    fun everyVendorHasCommonAndVendorSpecificSteps() {
        OemVendor.values().forEach { vendor ->
            val steps = OemHelp.stepsFor(vendor)
            assertEquals(OemHelp.commonSteps.size + 1, steps.size)
            assertEquals(OemHelp.commonSteps, steps.take(OemHelp.commonSteps.size))
            assertEquals(
                "vendor ${vendor.id} has no unique step",
                1,
                steps.drop(OemHelp.commonSteps.size).distinctBy { it.titleRes }.size,
            )
        }
    }

    @Test
    fun unknownVendorFallsBackToOther() {
        assertEquals(OemVendor.OTHER, OemVendor.from(null))
        assertEquals(OemVendor.OTHER, OemVendor.from("not-a-vendor"))
        assertEquals(OemVendor.PIXEL, OemVendor.from("pixel"))
        assertEquals(OemVendor.SAMSUNG, OemVendor.from("samsung"))
    }
}
