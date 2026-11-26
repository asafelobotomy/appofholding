// Settings › Defaults: what a newly added app starts with.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

ColumnLayout {
    id: tab

    /// The settings page (SettingsPage.qml): its state and what it does.
    property var host
    /// The policy chosen here, which "apply to all" on the page uses.
    readonly property alias policyIndex: defPolicy.currentIndex

    visible: tab.host.tabIndex === 2
    Layout.fillWidth: true
    spacing: Kirigami.Units.largeSpacing

    QQC2.Label {
        Layout.fillWidth: true
        wrapMode: Text.Wrap
        opacity: 0.7
        text: qsTr("What apps start with. Each app can still be changed on its own tab.host, and these also apply to apps added from the command line.")
    }

    SectionCard {
        title: qsTr("When a new version is found")
        iconName: "preferences-system-time"
        subtitle: qsTr("For apps whose update source is found from now on.")
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.smallSpacing
            QQC2.ComboBox {
                id: defPolicy
                Layout.fillWidth: true
                Layout.maximumWidth: Kirigami.Units.gridUnit * 20
                model: tab.host.policyLabels
                currentIndex: Math.max(0, tab.host.policyKeys.indexOf(tab.host.defs.policy || "notify"))
                onActivated: (i) => tab.host.saveDefault("policy", tab.host.policyKeys[i])
                Accessible.name: qsTr("Default update policy")
            }
            QQC2.Button {
                text: qsTr("Apply to all apps…")
                icon.name: "edit-select-all"
                enabled: tab.host.withSource > 0
                onClicked: tab.host.openApplyAll()
            }
        }
    }

    SectionCard {
        title: qsTr("Antivirus")
        iconName: "security-medium"
        subtitle: qsTr("oysterAV only looks. A file it flags stays where it is; you decide in oysterAV what happens to it. Needs oysterAV installed.")
        QQC2.ComboBox {
            id: defScan
            readonly property var keys: ["off", "warn", "block"]
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 20
            model: [qsTr("Off"), qsTr("Scan and warn me"), qsTr("Scan and stop a threat")]
            currentIndex: Math.max(0, keys.indexOf(tab.host.defs.scan || "off"))
            onActivated: (i) => tab.host.saveDefault("scan", keys[i])
            Accessible.name: qsTr("Scan AppImages with oysterAV")
        }
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.7
            text: defScan.currentIndex === 0 ? qsTr("AppImages are not scanned unless you ask.")
                : defScan.currentIndex === 1 ? qsTr("Each AppImage is scanned as it is added or updated, and the answer is kept. A threat is only reported.")
                : qsTr("As “Scan and warn me”, but an AppImage with a threat is not added or installed, and an update with one is thrown away.")
        }
    }

    SectionCard {
        title: qsTr("Sandbox")
        iconName: "security-high"
        subtitle: qsTr("For apps you add from now on. Apps already added keep their own sandbox.")
        QQC2.ComboBox {
            id: defSandbox
            readonly property var keys: ["none", "standard", "strict"]
            Layout.fillWidth: true
            Layout.maximumWidth: Kirigami.Units.gridUnit * 20
            model: [qsTr("Off"), qsTr("Standard"), qsTr("Strict")]
            currentIndex: Math.max(0, keys.indexOf(tab.host.defs.sandbox || "none"))
            onActivated: (i) => tab.host.saveDefault("sandbox", keys[i])
            Accessible.name: qsTr("Default sandbox")
        }
        QQC2.Label {
            Layout.fillWidth: true
            wrapMode: Text.Wrap
            opacity: 0.7
            text: [
                qsTr("New apps run like any other program."),
                qsTr("New apps work normally but can't see your files."),
                qsTr("New apps can't see your files, and have no network or sound.")
            ][defSandbox.currentIndex]
        }
    }
}
