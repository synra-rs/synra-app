package com.synra.app

import android.graphics.Color
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowInsets
import android.view.WindowManager
import android.webkit.WebView
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    hideSystemBars()
  }

  override fun onWindowFocusChanged(hasFocus: Boolean) {
    super.onWindowFocusChanged(hasFocus)
    if (hasFocus)
      hideSystemBars()
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    webView.settings.setSupportZoom(false)
    webView.settings.builtInZoomControls = false
    webView.settings.displayZoomControls = false
    webView.settings.textZoom = 100
    webView.settings.useWideViewPort = false
    webView.settings.loadWithOverviewMode = false
    webView.isHapticFeedbackEnabled = false
    webView.setInitialScale(0)
    webView.setOnTouchListener { _, event ->
      event.pointerCount > 1 || event.actionMasked == android.view.MotionEvent.ACTION_POINTER_DOWN
    }
  }

  private fun hideSystemBars() {
    window.setStatusBarColor(Color.TRANSPARENT)
    window.setNavigationBarColor(Color.TRANSPARENT)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
      window.isNavigationBarContrastEnforced = false
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
      window.setDecorFitsSystemWindows(false)
      window.insetsController?.let { controller ->
        controller.hide(WindowInsets.Type.statusBars() or WindowInsets.Type.navigationBars())
        controller.systemBarsBehavior = 2
      }
    }
    @Suppress("DEPRECATION")
    window.decorView.systemUiVisibility = (
      View.SYSTEM_UI_FLAG_FULLSCREEN
        or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
        or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
        or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
        or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
        or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
      )
    window.addFlags(WindowManager.LayoutParams.FLAG_FULLSCREEN)
  }
}
