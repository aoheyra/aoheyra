package com.aoheyra.app.notifications

import android.app.NotificationChannel
import android.app.NotificationManager
import android.content.Context
import com.aoheyra.app.R

/** Stable channel ids; changing one loses the user's notification settings. */
object NotificationChannels {
    const val MESH_SERVICE_ID = "mesh_service"
    const val MESSAGES_ID = "messages"
    const val LOCATION_SHARING_ID = "location_sharing"

    val definitions: List<ChannelDefinition> = listOf(
        ChannelDefinition(
            MESH_SERVICE_ID,
            R.string.channel_mesh_service_name,
            R.string.channel_mesh_service_description,
            NotificationManager.IMPORTANCE_LOW,
        ),
        ChannelDefinition(
            MESSAGES_ID,
            R.string.channel_messages_name,
            R.string.channel_messages_description,
            NotificationManager.IMPORTANCE_DEFAULT,
        ),
        ChannelDefinition(
            LOCATION_SHARING_ID,
            R.string.channel_location_sharing_name,
            R.string.channel_location_sharing_description,
            NotificationManager.IMPORTANCE_HIGH,
        ),
    )

    /** Idempotent: call from the foreground service before posting anything. */
    fun create(context: Context) {
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        definitions.forEach { definition ->
            val channel = NotificationChannel(
                definition.id,
                context.getString(definition.nameRes),
                definition.importance,
            )
            channel.description = context.getString(definition.descriptionRes)
            manager.createNotificationChannel(channel)
        }
    }
}

data class ChannelDefinition(
    val id: String,
    val nameRes: Int,
    val descriptionRes: Int,
    val importance: Int,
)
