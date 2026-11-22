package com.aoheyra.app.ui

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.aoheyra.app.R

/** Follows the system setting; dark theme is part of the accessibility bar. */
@Composable
fun AoHeyraTheme(content: @Composable () -> Unit) {
    val colors = if (isSystemInDarkTheme()) darkColorScheme() else lightColorScheme()
    MaterialTheme(colorScheme = colors, content = content)
}

/**
 * Text uses `sp` so the platform font scale is applied. Descriptions come from
 * resources so they can be translated and checked by check_resources.py.
 */
@Composable
fun CoreScreen(modifier: Modifier = Modifier) {
    val screenDescription = stringResource(R.string.cd_core_screen)
    Column(
        modifier = modifier
            .fillMaxSize()
            .semantics { contentDescription = screenDescription }
            .padding(16.dp),
    ) {
        Text(
            text = stringResource(R.string.core_title),
            style = MaterialTheme.typography.titleLarge,
            fontSize = 20.sp,
        )
        Text(
            text = stringResource(R.string.core_status),
            style = MaterialTheme.typography.bodyMedium,
            fontSize = 16.sp,
            modifier = Modifier.padding(top = 8.dp),
        )
    }
}
