import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// A scrolling page that wears the colour theme chosen in Settings.
Kirigami.ScrollablePage {
    Kirigami.Theme.inherit: applicationWindow().themePalette === null
    Kirigami.Theme.backgroundColor: applicationWindow().themePalette ? applicationWindow().themePalette.bg : "transparent"
    Kirigami.Theme.alternateBackgroundColor: applicationWindow().themePalette ? applicationWindow().themePalette.bgAlt : "transparent"
    Kirigami.Theme.textColor: applicationWindow().themePalette ? applicationWindow().themePalette.text : "transparent"
    Kirigami.Theme.disabledTextColor: applicationWindow().themePalette ? applicationWindow().themePalette.dim : "transparent"
    Kirigami.Theme.highlightColor: applicationWindow().themePalette ? applicationWindow().themePalette.accent : "transparent"
    Kirigami.Theme.highlightedTextColor: applicationWindow().themePalette ? applicationWindow().themePalette.accentText : "transparent"
    Kirigami.Theme.activeBackgroundColor: applicationWindow().themePalette ? applicationWindow().themePalette.bgActive : "transparent"
    Kirigami.Theme.hoverColor: applicationWindow().themePalette ? applicationWindow().themePalette.accent : "transparent"
    Kirigami.Theme.focusColor: applicationWindow().themePalette ? applicationWindow().themePalette.accent : "transparent"
    Kirigami.Theme.linkColor: applicationWindow().themePalette ? applicationWindow().themePalette.link : "transparent"
    Kirigami.Theme.positiveTextColor: applicationWindow().themePalette ? applicationWindow().themePalette.positive : "transparent"
    Kirigami.Theme.neutralTextColor: applicationWindow().themePalette ? applicationWindow().themePalette.neutral : "transparent"
    Kirigami.Theme.negativeTextColor: applicationWindow().themePalette ? applicationWindow().themePalette.negative : "transparent"
    palette.window: applicationWindow().themePalette ? applicationWindow().themePalette.bg : Kirigami.Theme.backgroundColor
    palette.windowText: applicationWindow().themePalette ? applicationWindow().themePalette.text : Kirigami.Theme.textColor
    palette.base: applicationWindow().themePalette ? applicationWindow().themePalette.bg : Kirigami.Theme.backgroundColor
    palette.text: applicationWindow().themePalette ? applicationWindow().themePalette.text : Kirigami.Theme.textColor
    palette.button: applicationWindow().themePalette ? applicationWindow().themePalette.bgAlt : Kirigami.Theme.backgroundColor
    palette.buttonText: applicationWindow().themePalette ? applicationWindow().themePalette.text : Kirigami.Theme.textColor
    palette.highlight: applicationWindow().themePalette ? applicationWindow().themePalette.accent : Kirigami.Theme.highlightColor
    palette.highlightedText: applicationWindow().themePalette ? applicationWindow().themePalette.accentText : Kirigami.Theme.highlightedTextColor
    palette.placeholderText: applicationWindow().themePalette ? applicationWindow().themePalette.dim : Kirigami.Theme.disabledTextColor
}
