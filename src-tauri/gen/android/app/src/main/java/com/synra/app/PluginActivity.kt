package com.synra.app

import android.os.Bundle
import android.util.TypedValue
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.Button
import android.widget.LinearLayout
import androidx.activity.OnBackPressedCallback
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

/** Host navigation lives outside plugin HTML and survives a broken or loading plugin page. */
class PluginActivity : TauriActivity() {
  private var pluginView: WebView? = null
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    onBackPressedDispatcher.addCallback(this, object : OnBackPressedCallback(true) {
      override fun handleOnBackPressed() = finish()
    })
  }

  override fun onWebViewCreate(webView: WebView) {
    super.onWebViewCreate(webView)
    pluginView = webView
    // Wry attaches the WebView after this callback; post until its parent exists.
    webView.post {
      val parent = webView.parent as? ViewGroup ?: return@post
      val index = parent.indexOfChild(webView)
      val layout = webView.layoutParams
      parent.removeView(webView)
      val container = LinearLayout(this).apply {
        orientation = LinearLayout.VERTICAL
        val color = TypedValue()
        theme.resolveAttribute(android.R.attr.colorBackground, color, true)
        setBackgroundColor(color.data)
      }
      val back = Button(this).apply {
        text = if (resources.configuration.locales[0].language == "zh") "‹ 返回 Synra" else "‹ Back to Synra"
        isAllCaps = false
        gravity = android.view.Gravity.START or android.view.Gravity.CENTER_VERTICAL
        setPadding((16 * resources.displayMetrics.density).toInt(), 0, 0, 0)
        setOnClickListener { finish() }
      }
      container.addView(back, LinearLayout.LayoutParams(
        ViewGroup.LayoutParams.MATCH_PARENT, (48 * resources.displayMetrics.density).toInt()
      ))
      container.addView(webView, LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, 0, 1f))
      parent.addView(container, index, layout)
      ViewCompat.setOnApplyWindowInsetsListener(container) { view, insets ->
        val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars())
        view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
        insets
      }
      ViewCompat.requestApplyInsets(container)
    }
  }

  override fun onPause() {
    pluginView?.evaluateJavascript("window.dispatchEvent(new Event('synra:host-hidden'))", null)
    super.onPause()
  }
}
