// The application object. A QApplication (Qt Widgets) rather than a QGuiApplication, because
// the tray icon (Qt.labs.platform's SystemTrayIcon) falls back to Qt Widgets on desktops
// without a StatusNotifier tray, such as XEmbed-only panels; the desktop style also prefers it.
#include <QtWidgets/QApplication>

namespace {
int g_argc = 0;
}

// argv must stay valid for the life of the application; the caller keeps it alive.
extern "C" void appofholding_app_new(int argc, char **argv) {
    g_argc = argc;
    new QApplication(g_argc, argv);
}

extern "C" int appofholding_app_exec() { return QApplication::exec(); }
