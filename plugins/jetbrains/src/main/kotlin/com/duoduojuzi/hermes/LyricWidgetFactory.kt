package com.duoduojuzi.hermes

import com.intellij.ide.plugins.PluginManagerCore
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.extensions.PluginId
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.CustomStatusBarWidget
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.util.ui.JBUI
import org.java_websocket.client.WebSocketClient
import org.java_websocket.handshake.ServerHandshake
import java.awt.Color
import java.awt.Font
import java.net.URI
import javax.swing.JComponent
import javax.swing.JLabel
import javax.swing.Timer

/**
 * @Author: DuoDuoJuZi
 * @Date: 2026-03-22
 */
class LyricWidgetFactory : StatusBarWidgetFactory {
    override fun getId(): String = "HermesLyricWidget"
    
    override fun getDisplayName(): String = "NetEase Lyrics"
    
    override fun isAvailable(project: Project): Boolean = true
    
    override fun createWidget(project: Project): StatusBarWidget = LyricWidget()
    
    override fun disposeWidget(widget: StatusBarWidget) {
        widget.dispose()
    }
    
    override fun canBeEnabledOn(statusBar: StatusBar): Boolean = true
}

class LyricWidget : CustomStatusBarWidget {

    private val label = JLabel("正在唤醒歌词引擎...").apply {
        font = Font(Font.SANS_SERIF, Font.BOLD, 13)
        border = JBUI.Borders.empty(0, 10)
    }

    private var webSocketClient: WebSocketClient? = null
    private var daemonProcess: Process? = null
    private var reconnectTimer: Timer? = null

    init {
        startRustDaemon()
        connectWebSocket()
    }

    override fun getComponent(): JComponent = label

    override fun ID(): String = "HermesLyricWidget"

    override fun install(statusBar: StatusBar) {}

    override fun dispose() {
        reconnectTimer?.stop()
        webSocketClient?.close()
    }

    private fun startRustDaemon() {
        try {
            val pluginId = PluginId.getId("com.duoduojuzi.hermes")
            val pluginPath = PluginManagerCore.getPlugin(pluginId)?.pluginPath ?: return

            val exeFile = pluginPath.resolve("bin/core.exe").toFile()

            if (exeFile.exists()) {
                val process = ProcessBuilder(exeFile.absolutePath)
                    .redirectErrorStream(true) 
                    .start()
                
                daemonProcess = process

                Thread {
                    try {
                        process.inputStream.bufferedReader().useLines { lines ->
                            lines.forEach { }
                        }
                    } catch (e: Exception) {
                    }
                }.start()
            }
        } catch (e: Exception) {
        }
    }

    private fun connectWebSocket() {
        if (webSocketClient?.isOpen == true) return

        webSocketClient = object : WebSocketClient(URI("ws://127.0.0.1:18333/ws")) {
            override fun onOpen(handshakedata: ServerHandshake?) {
                updateUI("歌词引擎就绪")
            }

            override fun onMessage(message: String?) {
                message?.let { updateUI("$it") }
            }

            override fun onClose(code: Int, reason: String?, remote: Boolean) {
                updateUI("歌词引擎已断开")
                reconnectTimer = Timer(3000) { connectWebSocket() }.apply { 
                    isRepeats = false 
                    start() 
                }
            }

            override fun onError(ex: Exception?) {}
        }
        webSocketClient?.connect()
    }

    private fun updateUI(text: String) {
        ApplicationManager.getApplication().invokeLater {
            label.text = text
            label.toolTipText = "网易云歌词 (Hermes)"
        }
    }
}
