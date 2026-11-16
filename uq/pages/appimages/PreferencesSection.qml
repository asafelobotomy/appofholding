// Preferences: the settings every AppImage shares, read with `appofholding config show` (followed
// from the first time the tab opens) and changed one key at a time with `config set`.
// appOFholding checks every value before it writes, so a refusal is shown under its row and
// nothing changes. The cards and words are the ones of the window's Settings.
import QtQuick
import QtQuick.Controls as QQC2
import QtQuick.Layouts
import org.uq.settings
import "logic.js" as Logic

ColumnLayout {
    id: section

    property var host
    property var view: null
    property var watching: null
    readonly property bool ready: section.view !== null && section.view.problem === ""
    readonly property bool idle: !!section.host && !section.host.busy

    function show(result) {
        const next = Logic.configView(result);
        if (next !== null)
            section.view = next;
    }

    function start() {
        if (section.host && section.host.ctx && section.watching === null)
            section.watching = section.host.ctx.command.watch("config", section.show);
    }

    /// Follows the settings from the first time the tab is shown, also when Settings opens the
    /// page on this tab.
    function autoload() {
        if (section.visible)
            section.start();
    }

    function save(key, value) {
        section.host.write("pref:" + key, "config-set", [key, String(value)]);
    }

    onVisibleChanged: section.autoload()

    Connections {
        target: section.host
        function onCtxChanged() {
            section.autoload();
        }
    }

    spacing: 16

    ErrorCard {
        title: "Couldn't read appOFholding's settings"
        detail: section.view ? section.view.problem : ""
        visible: section.view !== null && section.view.problem !== ""
        Layout.fillWidth: true
    }

    SettingsCard {
        title: "Background checks"
        iconName: "chronometer"
        help: "A system timer checks your apps even when appOFholding is closed. It installs updates for apps set to install automatically, and sends one notification per new version for apps set to “Tell me”."
        visible: section.ready
        Layout.fillWidth: true

        SettingRow {
            settingId: "schedule"
            label: "Check in the background"
            highlight: section.host ? section.host.highlight : ""
            Layout.fillWidth: true

            UqComboBox {
                id: schedule
                model: Logic.SCHEDULE_NAMES
                enabled: section.idle
                currentIndex: section.ready ? Logic.indexOfKey(Logic.SCHEDULE_KEYS, section.view.settings.schedule, 0) : 0
                onActivated: index => {
                    schedule.currentIndex = Qt.binding(() => section.ready ? Logic.indexOfKey(Logic.SCHEDULE_KEYS, section.view.settings.schedule, 0) : 0);
                    section.save("schedule", Logic.SCHEDULE_KEYS[index]);
                }
            }
        }
        Outcome {
            host: section.host
            noteKey: "pref:schedule"
            Layout.fillWidth: true
        }
    }

    SettingsCard {
        title: "Going back"
        iconName: "edit-undo"
        help: "Each update keeps the versions it replaces, so you can roll back if a new one misbehaves. Lowering the number deletes the extra ones now."
        visible: section.ready
        Layout.fillWidth: true

        SettingRow {
            settingId: "keep-versions"
            label: "Previous versions to keep"
            highlight: section.host ? section.host.highlight : ""
            Layout.fillWidth: true

            QQC2.SpinBox {
                id: keep
                from: 0
                to: section.ready ? section.view.maxKeep : 10
                editable: true
                enabled: section.idle
                value: section.ready ? section.view.settings.keep_versions : 2
                // Saved when the person stops changing it, so stepping 5 to 1 doesn't prune twice.
                onValueModified: keepTimer.restart()

                Timer {
                    id: keepTimer
                    interval: 800
                    onTriggered: {
                        if (section.ready && keep.value !== section.view.settings.keep_versions)
                            section.save("keep_versions", keep.value);
                        keep.value = Qt.binding(() => section.ready ? section.view.settings.keep_versions : 2);
                    }
                }
            }
        }
        Outcome {
            host: section.host
            noteKey: "pref:keep_versions"
            Layout.fillWidth: true
        }
    }

    SettingsCard {
        title: "When a new version is found"
        iconName: "preferences-system-time"
        help: "For apps whose update source is found from now on."
        visible: section.ready
        Layout.fillWidth: true

        SettingRow {
            settingId: "policy"
            label: "Update policy for new apps"
            highlight: section.host ? section.host.highlight : ""
            Layout.fillWidth: true

            UqComboBox {
                id: policy
                model: Logic.POLICY_NAMES
                implicitWidth: 230
                enabled: section.idle
                currentIndex: section.ready ? Logic.indexOfKey(Logic.POLICY_KEYS, section.view.settings.policy, 1) : 1
                onActivated: index => {
                    policy.currentIndex = Qt.binding(() => section.ready ? Logic.indexOfKey(Logic.POLICY_KEYS, section.view.settings.policy, 1) : 1);
                    section.save("policy", Logic.POLICY_KEYS[index]);
                }
            }
            QQC2.Button {
                text: "Apply to all apps…"
                icon.name: "edit-select-all"
                enabled: section.idle && section.ready && section.view.withSource > 0
                onClicked: {
                    if (section.ready)
                        section.host.write("pref:policy", "config-policy-all", [section.view.settings.policy]);
                }
            }
        }
        Outcome {
            host: section.host
            noteKey: "pref:policy"
            Layout.fillWidth: true
        }
    }

    SettingsCard {
        title: "Sandbox"
        iconName: "security-high"
        help: "For apps you add from now on. Apps already added keep their own sandbox."
        visible: section.ready
        Layout.fillWidth: true

        SettingRow {
            settingId: "sandbox"
            label: "Sandbox for new apps"
            highlight: section.host ? section.host.highlight : ""
            Layout.fillWidth: true

            UqComboBox {
                id: sandbox
                model: Logic.SANDBOX_NAMES
                enabled: section.idle
                currentIndex: section.ready ? Logic.indexOfKey(Logic.SANDBOX_KEYS, section.view.settings.sandbox, 0) : 0
                onActivated: index => {
                    sandbox.currentIndex = Qt.binding(() => section.ready ? Logic.indexOfKey(Logic.SANDBOX_KEYS, section.view.settings.sandbox, 0) : 0);
                    section.save("sandbox", Logic.SANDBOX_KEYS[index]);
                }
            }
        }
        UqLabel {
            text: Logic.SANDBOX_HELP[sandbox.currentIndex]
            wrapMode: Text.Wrap
            opacity: 0.7
            Layout.fillWidth: true
        }
        Outcome {
            host: section.host
            noteKey: "pref:sandbox"
            Layout.fillWidth: true
        }
    }

    SettingsCard {
        title: "Antivirus"
        iconName: "security-medium"
        help: "oysterAV only looks. A file it flags stays where it is; you decide in oysterAV what happens to it. Needs oysterAV installed."
        visible: section.ready
        Layout.fillWidth: true

        SettingRow {
            settingId: "scan"
            label: "Scan AppImages with oysterAV"
            highlight: section.host ? section.host.highlight : ""
            Layout.fillWidth: true

            UqComboBox {
                id: scan
                objectName: "scan-mode"
                model: Logic.SCAN_NAMES
                implicitWidth: 230
                enabled: section.idle
                currentIndex: section.ready ? Logic.indexOfKey(Logic.SCAN_KEYS, section.view.settings.scan, 0) : 0
                onActivated: index => {
                    scan.currentIndex = Qt.binding(() => section.ready ? Logic.indexOfKey(Logic.SCAN_KEYS, section.view.settings.scan, 0) : 0);
                    section.save("scan", Logic.SCAN_KEYS[index]);
                }
            }
        }
        UqLabel {
            text: Logic.SCAN_HELP[scan.currentIndex]
            wrapMode: Text.Wrap
            opacity: 0.7
            Layout.fillWidth: true
        }
        Outcome {
            host: section.host
            noteKey: "pref:scan"
            Layout.fillWidth: true
        }
    }

    SettingsCard {
        title: "More in appOFholding"
        iconName: "io.github.appofholding"
        help: "Appearance, the library folder, the tray and each app's launch options and sandbox are in its own window."
        Layout.fillWidth: true

        QQC2.Button {
            text: "Open appOFholding"
            icon.name: "window-new"
            onClicked: section.host.ctx.openApp("io.github.appofholding.desktop")
        }
    }
}
