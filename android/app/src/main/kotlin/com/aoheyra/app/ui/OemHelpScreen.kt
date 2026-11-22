package com.aoheyra.app.ui

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.aoheyra.app.R
import com.aoheyra.app.onboarding.OemHelp
import com.aoheyra.app.onboarding.OemVendor

/** Onboarding help for the vendor detected by AND-03; unknown ids use OTHER. */
@Composable
fun OemHelpScreen(
    vendorId: String?,
    modifier: Modifier = Modifier,
) {
    val vendor = OemVendor.from(vendorId)
    val steps = OemHelp.stepsFor(vendor)
    val listDescription = stringResource(R.string.cd_oem_help_list)
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .semantics { contentDescription = listDescription },
    ) {
        item {
            Text(
                text = stringResource(vendor.labelRes),
                style = MaterialTheme.typography.headlineSmall,
                modifier = Modifier.padding(16.dp),
            )
        }
        items(steps) { step ->
            Text(
                text = stringResource(step.titleRes),
                style = MaterialTheme.typography.titleMedium,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp),
            )
            Text(
                text = stringResource(step.detailRes),
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, bottom = 12.dp),
            )
        }
    }
}
