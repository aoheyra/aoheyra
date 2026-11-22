package com.aoheyra.app.onboarding

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import com.aoheyra.app.R

/**
 * Explains each missing capability before asking for it and gives a working
 * recovery action after a denial or revocation. The card count is bounded by
 * [OnboardingState.MAX_ISSUES]; no state is accumulated in the composable.
 */
@Composable
fun OnboardingScreen(
    state: OnboardingState,
    onAction: (RecoveryAction) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
    ) {
        Text(
            text = stringResource(R.string.onboarding_title),
            style = MaterialTheme.typography.headlineMedium,
        )
        Spacer(Modifier.height(8.dp))
        Text(
            text = stringResource(R.string.onboarding_intro),
            style = MaterialTheme.typography.bodyMedium,
        )
        if (state.notice == Notice.SETTINGS_UNAVAILABLE) {
            Spacer(Modifier.height(12.dp))
            Text(
                text = stringResource(R.string.notice_settings_unavailable),
                color = MaterialTheme.colorScheme.error,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        state.issues().forEach { issue ->
            Spacer(Modifier.height(12.dp))
            Card(modifier = Modifier.fillMaxWidth()) {
                Column(modifier = Modifier.padding(16.dp)) {
                    val (title, body) = issueText(issue)
                    Text(text = title, style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(4.dp))
                    Text(text = body, style = MaterialTheme.typography.bodyMedium)
                    Spacer(Modifier.height(12.dp))
                    Button(
                        onClick = { onAction(issue.action) },
                        modifier = Modifier.fillMaxWidth(),
                    ) {
                        Text(text = actionLabel(issue.action))
                    }
                }
            }
        }
    }
}

@Composable
private fun issueText(issue: RecoveryIssue): Pair<String, String> {
    val subject = when (issue.capability) {
        Capability.BLUETOOTH -> stringResource(R.string.capability_bluetooth)
        Capability.LOCATION -> stringResource(R.string.capability_location)
        Capability.NOTIFICATIONS -> stringResource(R.string.capability_notifications)
        null -> stringResource(R.string.capability_battery)
    }
    return when (issue.reason) {
        IssueReason.FIRST_EXPLANATION ->
            stringResource(R.string.issue_first_title, subject) to
                stringResource(R.string.issue_first_body, capabilityExplanation(issue.capability))

        IssueReason.DENIED_WITH_RATIONALE ->
            stringResource(R.string.issue_denied_title, subject) to
                stringResource(R.string.issue_denied_body, capabilityExplanation(issue.capability))

        IssueReason.PERMANENTLY_DENIED ->
            stringResource(R.string.issue_permanent_title, subject) to
                stringResource(R.string.issue_permanent_body, capabilityExplanation(issue.capability))

        IssueReason.BLUETOOTH_OFF ->
            stringResource(R.string.issue_bluetooth_off_title) to
                stringResource(R.string.issue_bluetooth_off_body)

        IssueReason.LOCATION_OFF ->
            stringResource(R.string.issue_location_off_title) to
                stringResource(R.string.issue_location_off_body)

        IssueReason.BATTERY_OPTIMIZED ->
            stringResource(R.string.issue_battery_title) to
                stringResource(R.string.issue_battery_body)
    }
}

@Composable
private fun capabilityExplanation(capability: Capability?): String = when (capability) {
    Capability.BLUETOOTH -> stringResource(R.string.explain_bluetooth)
    Capability.LOCATION -> stringResource(R.string.explain_location)
    Capability.NOTIFICATIONS -> stringResource(R.string.explain_notifications)
    null -> stringResource(R.string.explain_system)
}

@Composable
private fun actionLabel(action: RecoveryAction): String = when (action) {
    is RecoveryAction.RequestPermissions -> stringResource(R.string.action_review_permission)
    RecoveryAction.OpenAppSettings -> stringResource(R.string.action_app_settings)
    RecoveryAction.OpenBluetoothSettings -> stringResource(R.string.action_bluetooth_settings)
    RecoveryAction.OpenLocationSettings -> stringResource(R.string.action_location_settings)
    RecoveryAction.OpenBatterySettings -> stringResource(R.string.action_battery_settings)
}
