package com.bandear.android

import android.graphics.Color
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.runtime.getValue
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.bandear.android.ui.screens.MonitorApp
import com.bandear.android.ui.state.MonitorViewModel
import com.bandear.android.ui.theme.BandEarTheme

/**
 * The app's only screen: it shows [MonitorApp] and binds it to the [MonitorViewModel].
 *
 * The session, the mix and everything the screens show live in the ViewModel, so turning the
 * phone keeps the audio and the control channel going; they end when the ViewModel is cleared,
 * that is when the Activity finishes for good, as the old screen ended them in `onDestroy`.
 */
class MainActivity : ComponentActivity() {
    private val monitor: MonitorViewModel by viewModels { MonitorViewModel.Factory }

    override fun onCreate(savedInstanceState: Bundle?) {
        // The app draws behind the system bars. Its look is dark only, so the bars keep light icons
        // whatever theme the phone itself uses.
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        setContent {
            BandEarTheme {
                val state by monitor.state.collectAsStateWithLifecycle()
                MonitorApp(state = state, actions = monitor)
            }
        }
    }
}
