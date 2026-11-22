package com.aoheyra.app.notifications

import android.app.NotificationManager
import org.junit.Assert.assertEquals
import org.junit.Test

/** AND-21: channel ids are stable and each channel has the expected importance. */
class NotificationChannelsTest {
    @Test
    fun channelDefinitionsAreUnique() {
        val ids = NotificationChannels.definitions.map { it.id }
        assertEquals(3, ids.size)
        assertEquals(3, ids.toSet().size)
    }

    @Test
    fun importanceMatchesTheChannelPurpose() {
        val definitions = NotificationChannels.definitions.associateBy { it.id }
        assertEquals(
            NotificationManager.IMPORTANCE_LOW,
            definitions.getValue(NotificationChannels.MESH_SERVICE_ID).importance,
        )
        assertEquals(
            NotificationManager.IMPORTANCE_DEFAULT,
            definitions.getValue(NotificationChannels.MESSAGES_ID).importance,
        )
        assertEquals(
            NotificationManager.IMPORTANCE_HIGH,
            definitions.getValue(NotificationChannels.LOCATION_SHARING_ID).importance,
        )
    }
}
