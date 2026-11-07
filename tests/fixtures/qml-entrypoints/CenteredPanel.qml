import QtQuick

// The production CenteredPanel is a layer-shell PanelWindow, which Qt's
// offscreen platform intentionally cannot construct. This API-compatible test
// double lets the entrypoint fixture validate the widget tree and bindings
// without a compositor; the real CenteredPanel.qml is used at runtime.
Item {
  id: root

  required property Item anchorItem
  required property QtObject bar
  property var owner: null
  property bool open: false
  property Item focusTarget: null
  property bool centerOnBar: false
  property int padding: 14
  property color cardColor: "white"
  readonly property real screenH: 1080
  readonly property real verticalContentInset: padding * 2
  property int contentWidth: 1
  property int contentHeight: 1
  default property alias contentItem: holder.children

  function fittedContentWidth(value, cap) {
    return Math.max(1, Math.min(Number(value) || 1, Number(cap) || Number(value) || 1))
  }

  function cappedContentHeight(value) {
    return Math.max(1, Number(value) || 1)
  }

  width: contentWidth
  height: contentHeight

  Item {
    id: holder
    anchors.fill: parent
  }
}
