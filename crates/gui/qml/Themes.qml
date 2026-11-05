pragma Singleton
import QtQuick

// Colour themes the user can pick. "system" follows the desktop. The Gruvbox palettes are also in
// theme.cpp, which gives them to the style's controls; here they are for Kirigami. Gruvbox by Pavel Pertsev.
QtObject {
    readonly property var names: ["system", "gruvbox-dark", "gruvbox-light"]
    readonly property var labels: [qsTr("Follow the system"), qsTr("Gruvbox dark"), qsTr("Gruvbox light")]

    readonly property var gruvboxDark: ({
        bg: "#282828", bgAlt: "#3c3836", bgActive: "#504945", text: "#ebdbb2", dim: "#a89984",
        accent: "#d79921", accentText: "#282828", link: "#83a598", visited: "#d3869b",
        positive: "#b8bb26", neutral: "#fe8019", negative: "#fb4934"
    })
    readonly property var gruvboxLight: ({
        bg: "#fbf1c7", bgAlt: "#ebdbb2", bgActive: "#d5c4a1", text: "#3c3836", dim: "#7c6f64",
        accent: "#b57614", accentText: "#fbf1c7", link: "#076678", visited: "#8f3f71",
        positive: "#79740e", neutral: "#af3a03", negative: "#9d0006"
    })

    // The palette for a theme name, or null to follow the system.
    function palette(name) {
        return name === "gruvbox-dark" ? gruvboxDark : name === "gruvbox-light" ? gruvboxLight : null
    }
}
