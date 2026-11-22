package com.aoheyra.app.service

import android.content.Context

/** Persists only the explicit-stop lifecycle flag, never application data. */
interface StopFlagStore {
    /** True while the user or runtime explicitly stopped the mesh. */
    fun isStopped(): Boolean

    /** Records an explicit stop. */
    fun setStopped(stopped: Boolean)
}

/** SharedPreferences-backed lifecycle flag. */
class SharedPreferencesStopFlagStore(context: Context) : StopFlagStore {
    private val preferences = context.applicationContext.getSharedPreferences(
        PREFS_NAME,
        Context.MODE_PRIVATE,
    )

    override fun isStopped(): Boolean = preferences.getBoolean(KEY_STOPPED, false)

    override fun setStopped(stopped: Boolean) {
        preferences.edit().putBoolean(KEY_STOPPED, stopped).apply()
    }

    private companion object {
        const val PREFS_NAME: String = "mesh_service_lifecycle"
        const val KEY_STOPPED: String = "explicitly_stopped"
    }
}
