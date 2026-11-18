package io.github.rainwu0.paperreader

import android.os.Bundle
import android.view.View
import androidx.activity.enableEdgeToEdge
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
    followKeyboard()
  }

  /**
   * 軟鍵盤：滿版（edge-to-edge）時系統不替應用縮小畫面，鍵盤直接蓋在網頁上，
   * 網頁也量不到鍵盤（visualViewport 不變）。照 Android 官方處理滿版的做法讀鍵盤的邊界（WindowInsets 的 ime），
   * 把內容區的下緣墊到鍵盤上緣：網頁跟著變矮，底部的面板與按鈕留在鍵盤上方（效果等於 adjustResize）。
   * 手勢列的留白網頁自己會留（`--inset-bottom`），所以只墊鍵盤比手勢列高出的那一段。
   * 浮動的鍵盤不占邊界，這裡就不墊。
   */
  private fun followKeyboard() {
    val content = findViewById<View>(android.R.id.content)
    ViewCompat.setOnApplyWindowInsetsListener(content) { v, insets ->
      val ime = insets.getInsets(WindowInsetsCompat.Type.ime()).bottom
      val bars = insets.getInsets(WindowInsetsCompat.Type.systemBars()).bottom
      v.setPadding(0, 0, 0, maxOf(0, ime - bars))
      insets
    }
  }
}
