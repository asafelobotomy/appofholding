import QtQuick
import QtQuick.Controls as QQC2
import org.kde.kirigami as Kirigami

// A scrolling page that wears the colour theme chosen in Settings.
Kirigami.ScrollablePage {
    id: page
    readonly property ThemeBindings themeBindings: ThemeBindings { target: page }
}
