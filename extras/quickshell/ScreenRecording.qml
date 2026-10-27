import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Widgets

// Stop button shown while pinhole is recording, like the one in the macOS menu bar.
// pinhole writes $XDG_RUNTIME_DIR/pinhole/status.json when a recording starts and stops.
Item {
    id: root

    property bool recording: false

    visible: recording
    implicitWidth: icon.width
    implicitHeight: icon.height

    FileView {
        id: status
        path: Quickshell.env("XDG_RUNTIME_DIR") + "/pinhole/status.json"
        printErrors: false
        watchChanges: true
        onFileChanged: reload()
        onLoaded: {
            try {
                root.recording = JSON.parse(text()).recording === true
            } catch (e) {
                root.recording = false
            }
        }
        onLoadFailed: root.recording = false
    }

    // The status file may not exist until pinhole first runs, which file watching can miss.
    Timer {
        interval: 2000
        running: true
        repeat: true
        onTriggered: status.reload()
    }

    Process {
        id: stop
        command: ["pinhole", "stop"]
    }

    IconImage {
        id: icon
        width: 20
        height: 20
        source: Qt.resolvedUrl("icons/screen-recording-stop.svg")
        opacity: mouse.pressed ? 0.5 : mouse.containsMouse ? 0.8 : 1
    }

    MouseArea {
        id: mouse
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: stop.running = true
    }
}
