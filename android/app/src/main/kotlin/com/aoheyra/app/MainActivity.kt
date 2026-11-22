package com.aoheyra.app

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.ui.Modifier
import com.aoheyra.app.ui.AoHeyraTheme
import com.aoheyra.app.ui.CoreScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            AoHeyraTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    CoreScreen()
                }
            }
        }
    }
}
