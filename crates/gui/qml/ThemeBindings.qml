import QtQuick
import org.kde.kirigami as Kirigami

// Puts the colour theme chosen in Settings on `target` (its Kirigami colours and its control
// palette). With "follow the system" it sets nothing at all, so the desktop's colours are
// inherited untouched: assigning any Kirigami colour, even a fallback, would stop that.
QtObject {
    id: tb
    required property QtObject target
    // The window passes its palette in directly; pages and drawers look it up.
    property var p: applicationWindow() ? applicationWindow().themePalette : null
    readonly property bool on: !!p
    readonly property QtObject theme: target.Kirigami.Theme
    readonly property QtObject palette: target.palette

    property list<QtObject> bindings: [
        Binding { when: tb.on; target: tb.theme; property: "inherit"; value: false },
        Binding { when: tb.on; target: tb.theme; property: "backgroundColor"; value: tb.p ? tb.p.bg : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "alternateBackgroundColor"; value: tb.p ? tb.p.bgAlt : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "textColor"; value: tb.p ? tb.p.text : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "disabledTextColor"; value: tb.p ? tb.p.dim : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "highlightColor"; value: tb.p ? tb.p.accent : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "highlightedTextColor"; value: tb.p ? tb.p.accentText : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "activeBackgroundColor"; value: tb.p ? tb.p.bgActive : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "hoverColor"; value: tb.p ? tb.p.accent : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "focusColor"; value: tb.p ? tb.p.accent : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "linkColor"; value: tb.p ? tb.p.link : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "positiveTextColor"; value: tb.p ? tb.p.positive : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "neutralTextColor"; value: tb.p ? tb.p.neutral : "transparent" },
        Binding { when: tb.on; target: tb.theme; property: "negativeTextColor"; value: tb.p ? tb.p.negative : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "window"; value: tb.p ? tb.p.bg : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "windowText"; value: tb.p ? tb.p.text : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "base"; value: tb.p ? tb.p.bg : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "alternateBase"; value: tb.p ? tb.p.bgAlt : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "text"; value: tb.p ? tb.p.text : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "button"; value: tb.p ? tb.p.bgAlt : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "buttonText"; value: tb.p ? tb.p.text : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "highlight"; value: tb.p ? tb.p.accent : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "highlightedText"; value: tb.p ? tb.p.accentText : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "placeholderText"; value: tb.p ? tb.p.dim : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "toolTipBase"; value: tb.p ? tb.p.bgAlt : "transparent" },
        Binding { when: tb.on; target: tb.palette; property: "toolTipText"; value: tb.p ? tb.p.text : "transparent" }
    ]
}
