// Colin McRae Rally launcher for Linux (Quickshell).
// Run: qs -p <this folder>   (see linux/README.md)
// Choices are read from launcher.cfg (and ~/.config/cmr-launcher/scaler) when it opens
// and saved there when you press Start.
//
// Look: the 555 Impreza livery from the game's cover — rally blue, one gold
// accent for whatever is active, a decal-style condensed italic title.
import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io

ShellRoot {
  id: root
  // cmr-play sits one folder up, next to CMRLauncher.exe.
  readonly property string helper: Qt.resolvedUrl("../cmr-play").toString().replace("file://", "")

  // Palette
  readonly property color cBlue: "#0f2e6e"
  readonly property color cPanel: "#163a85"
  readonly property color cLine: "#2b4f9e"
  readonly property color cGold: "#f5c400"
  readonly property color cInk: "#eef2fb"
  readonly property color cMuted: "#a7b6d9"
  readonly property color cOnGold: "#10244f"
  readonly property string family: "Noto Sans"

  readonly property var resolutions: [
    { key: "original", label: "Original" },
    { key: "1440x1080", label: "1440×1080" },
    { key: "1920x1080", label: "1080p" },
    { key: "2560x1440", label: "1440p" },
    { key: "3840x2160", label: "4K" }
  ]
  readonly property var distances: [
    { key: "1.0", label: "1×" }, { key: "1.25", label: "1.25×" }, { key: "1.5", label: "1.5×" },
    { key: "2.0", label: "2×" }, { key: "2.5", label: "2.5×" }, { key: "3.0", label: "3×" }
  ]
  // gamescope filters, sharpest first
  readonly property var scalers: [
    { key: "nearest", label: "Sharp" },
    { key: "pixel", label: "Even pixels" },
    { key: "fsr", label: "FSR" },
    { key: "linear", label: "Smooth" }
  ]
  readonly property var scalerNotes: ({
    nearest: "Every game pixel becomes a crisp block. Best for the 1998 artwork.",
    pixel: "Crisp blocks, with only their edges blended so all are the same size.",
    fsr: "AMD FSR: smooth, then sharpened. Softer than Sharp.",
    linear: "Plain smoothing. The softest, most blurred look."
  })

  property string status: "idle"
  property string message: ""
  property bool loaded: false

  function find(list, key, fallback) {
    const i = list.findIndex(o => o.key === key);
    return i < 0 ? fallback : i;
  }

  function apply(text) {
    const kv = {};
    for (const l of text.split("\n")) {
      const at = l.indexOf("=");
      if (at > 0) kv[l.slice(0, at)] = l.slice(at + 1).trim();
    }
    res.current = find(resolutions, kv.resolution || "original", 0);
    dist.current = find(distances, kv.draw_distance || "1.0", 0);
    scaler.current = find(scalers, kv.scaler || "nearest", 0);
    wide.checked = kv.widescreen === "true";
    quit.checked = kv.close_quits !== "false";
    pad.checked = kv.gamepad_axes !== "false";
    saves.text = kv.saves || "";
    loaded = true;
  }

  Process {
    command: [root.helper, "get"]
    running: true
    stdout: StdioCollector { onStreamFinished: root.apply(text) }
  }
  Process {
    id: action
    stdout: StdioCollector { onStreamFinished: root.message = text.trim() }
    onExited: statusPoll.running = true
  }
  Process {
    id: statusPoll
    command: [root.helper, "status"]
    stdout: StdioCollector { onStreamFinished: root.status = text.trim() }
  }
  Timer {
    interval: 2000; running: true; repeat: true; triggeredOnStart: true
    onTriggered: if (!statusPoll.running) statusPoll.running = true
  }

  // A row of choices; the chosen one is gold.
  component Chips: Row {
    id: chips
    property var options: []
    property int current: 0
    spacing: 6
    Repeater {
      model: chips.options
      delegate: Rectangle {
        required property var modelData
        required property int index
        readonly property bool on: index === chips.current
        width: label.implicitWidth + 24; height: 32; radius: 6
        color: on ? root.cGold : (hover.hovered ? root.cLine : "transparent")
        border.color: on ? root.cGold : root.cLine
        border.width: 1
        Text {
          id: label
          anchors.centerIn: parent
          text: modelData.label
          color: parent.on ? root.cOnGold : root.cInk
          font.family: root.family; font.pixelSize: 13; font.weight: parent.on ? Font.DemiBold : Font.Normal
        }
        HoverHandler { id: hover; cursorShape: Qt.PointingHandCursor }
        TapHandler { onTapped: chips.current = index }
      }
    }
  }

  // A switch: knob left and outlined = off, knob right on gold = on.
  component Toggle: Item {
    id: t
    property bool checked: false
    property string text: ""
    implicitWidth: row.implicitWidth; implicitHeight: 32
    Row {
      id: row
      spacing: 12
      anchors.verticalCenter: parent.verticalCenter
      Rectangle {
        width: 46; height: 26; radius: 13
        color: t.checked ? root.cGold : "transparent"
        border.color: t.checked ? root.cGold : root.cMuted
        border.width: 1.5
        Behavior on color { ColorAnimation { duration: 140 } }
        Rectangle {
          width: 18; height: 18; radius: 9; y: 4
          x: t.checked ? parent.width - width - 4 : 4
          color: t.checked ? root.cOnGold : root.cMuted
          Behavior on x { NumberAnimation { duration: 140; easing.type: Easing.OutCubic } }
        }
      }
      Text {
        anchors.verticalCenter: parent.verticalCenter
        text: t.text
        color: root.cInk; font.family: root.family; font.pixelSize: 14
      }
    }
    HoverHandler { cursorShape: Qt.PointingHandCursor }
    TapHandler { onTapped: t.checked = !t.checked }
  }

  component Heading: Text {
    color: root.cMuted
    font.family: root.family; font.pixelSize: 13; font.weight: Font.DemiBold
    Layout.topMargin: 6
  }
  component FieldLabel: Text {
    color: root.cInk; font.family: root.family; font.pixelSize: 14
    Layout.preferredWidth: 120
  }

  FloatingWindow {
    title: "Colin McRae Rally"
    implicitWidth: 900
    implicitHeight: 660
    color: root.cBlue

    // Colin, on the left: the photo fills the side, a blue fade joins it to
    // the controls.
    Image {
      id: photo
      anchors.left: parent.left; anchors.top: parent.top; anchors.bottom: parent.bottom
      width: 320
      source: "img/portrait.jpg"
      fillMode: Image.PreserveAspectCrop
      horizontalAlignment: Image.AlignLeft
      Rectangle {
        anchors.fill: parent
        gradient: Gradient {
          orientation: Gradient.Horizontal
          GradientStop { position: 0.55; color: "transparent" }
          GradientStop { position: 1.0; color: root.cBlue }
        }
      }
      Rectangle {
        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
        height: 120
        gradient: Gradient {
          GradientStop { position: 0.0; color: "transparent" }
          GradientStop { position: 1.0; color: root.cBlue }
        }
      }
      Column {
        anchors.left: parent.left; anchors.bottom: parent.bottom; anchors.margins: 20
        spacing: 2
        Text { text: "Colin McRae"; color: root.cInk; font.family: root.family; font.pixelSize: 18; font.weight: Font.Bold }
        Text { text: "1968–2007 · World Rally Champion 1995"; color: root.cMuted; font.family: root.family; font.pixelSize: 12 }
        Text { text: "Photo: Carolyn Williams, CC BY 2.0"; color: root.cMuted; font.family: root.family; font.pixelSize: 10; opacity: 0.8 }
      }
    }

    // The decal: two slanted gold livery stripes behind the title.
    Item {
      anchors.right: parent.right; anchors.top: parent.top
      width: 260; height: 112; clip: true
      Repeater {
        model: 2
        Rectangle {
          required property int index
          width: 34 - index * 14; height: 220
          x: 150 + index * 46; y: -50
          rotation: 28
          color: root.cGold
          opacity: 1 - index * 0.35
        }
      }
    }

    Flickable {
      id: pane
      anchors.top: parent.top; anchors.bottom: parent.bottom; anchors.right: parent.right
      anchors.left: photo.right
      contentHeight: Math.max(height, form.implicitHeight + 56)
      clip: true
      boundsBehavior: Flickable.StopAtBounds
      ScrollBar.vertical: ScrollBar { policy: pane.contentHeight > pane.height ? ScrollBar.AsNeeded : ScrollBar.AlwaysOff }

    ColumnLayout {
      id: form
      x: 8; y: 28
      width: pane.width - 36
      height: pane.contentHeight - 56
      spacing: 12

      Text {
        text: "Colin McRae Rally"
        color: root.cInk
        font.family: "Noto Sans ExtraCondensed"; font.pixelSize: 44; font.weight: Font.Black
        font.italic: true
      }
      Text {
        text: root.status === "running" ? "On the stage. Super+W or Stop ends it." : "Ready."
        color: root.status === "running" ? root.cGold : root.cMuted
        font.family: root.family; font.pixelSize: 13
        Layout.topMargin: -8
      }

      Heading { text: "Picture" }
      GridLayout {
        columns: 2; columnSpacing: 16; rowSpacing: 12
        FieldLabel { text: "Race resolution" }
        Chips { id: res; options: root.resolutions }
        FieldLabel { text: "Draw distance" }
        Chips { id: dist; options: root.distances }
        FieldLabel { text: "Scaler" }
        ColumnLayout {
          spacing: 6
          Chips { id: scaler; options: root.scalers }
          Text {
            text: root.scalerNotes[root.scalers[scaler.current].key]
            color: root.cMuted; font.family: root.family; font.pixelSize: 12
            wrapMode: Text.WordWrap; Layout.maximumWidth: 400
          }
        }
        Item { width: 1; height: 1 }
        Toggle { id: wide; text: "Widescreen: show more to the sides" }
      }

      Heading { text: "Playing" }
      ColumnLayout {
        spacing: 6
        Toggle { id: pad; checked: true; text: "Gamepad triggers, right stick and Start pause" }
        Toggle { id: quit; checked: true; text: "Closing the window quits the game" }
      }

      Heading { text: "Saves" }
      TextField {
        id: saves
        Layout.fillWidth: true
        placeholderText: "Saved Games\\Colin McRae Rally (default)"
        color: root.cInk; placeholderTextColor: root.cMuted
        font.family: root.family; font.pixelSize: 14
        leftPadding: 12; rightPadding: 12
        background: Rectangle {
          implicitHeight: 36; radius: 6
          color: root.cPanel
          border.color: saves.activeFocus ? root.cGold : root.cLine
        }
      }
      Text {
        text: "A Windows path, e.g. Z:\\home\\you\\rally-saves. Leave empty for the default."
        color: root.cMuted; font.family: root.family; font.pixelSize: 12
        Layout.topMargin: -6
      }

      Item { Layout.fillHeight: true }

      RowLayout {
        Layout.fillWidth: true
        spacing: 14
        Text {
          Layout.fillWidth: true
          text: "In the game keep Resolution on 640×480; the race resolution replaces it."
          color: root.cMuted; font.family: root.family; font.pixelSize: 12
          wrapMode: Text.WordWrap
        }
        Rectangle {
          id: startStop
          readonly property bool running: root.status === "running"
          width: 150; height: 46; radius: 8
          color: running ? "transparent" : (root.loaded ? root.cGold : root.cLine)
          border.color: running ? root.cInk : root.cGold
          border.width: running ? 1.5 : 0
          Text {
            anchors.centerIn: parent
            text: startStop.running ? "Stop" : "Start"
            color: startStop.running ? root.cInk : root.cOnGold
            font.family: "Noto Sans ExtraCondensed"; font.pixelSize: 20; font.weight: Font.Black
            font.italic: true
          }
          HoverHandler { cursorShape: Qt.PointingHandCursor }
          TapHandler {
            enabled: root.loaded
            onTapped: {
              if (startStop.running) {
                action.command = [root.helper, "stop"];
              } else {
                action.command = [root.helper, "play",
                  "resolution=" + root.resolutions[res.current].key,
                  "widescreen=" + (wide.checked ? "true" : ""),
                  "draw_distance=" + (dist.current === 0 ? "" : root.distances[dist.current].key),
                  "scaler=" + root.scalers[scaler.current].key,
                  "close_quits=" + (quit.checked ? "true" : "false"),
                  "gamepad_axes=" + (pad.checked ? "true" : "false"),
                  "saves=" + saves.text.trim()];
              }
              action.running = true;
            }
          }
        }
      }
    }
  }
  }
}
