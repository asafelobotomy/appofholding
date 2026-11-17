// Colour themes: sets the application palette. Called from Rust before the interface is created.
#include <QtGui/QColor>
#include <QtGui/QGuiApplication>
#include <QtGui/QIcon>
#include <QtCore/QJsonDocument>
#include <QtCore/QFile>
#include <QtCore/QJsonObject>
#include <QtGui/QPalette>

namespace {
struct Colors {
    QColor bg, bgAlt, bgActive, text, dim, accent, accentText, link, visited;
};

const Colors dark{"#282828", "#3c3836", "#504945", "#ebdbb2", "#a89984",
                      "#d79921", "#282828", "#83a598", "#d3869b"};
const Colors light{"#fbf1c7", "#ebdbb2", "#d5c4a1", "#3c3836", "#7c6f64",
                       "#b57614", "#fbf1c7", "#076678", "#8f3f71"};

QPalette make(const Colors &c) {
    QPalette p;
    const QColor bg = c.bg, alt = c.bgAlt, active = c.bgActive, text = c.text, dim = c.dim;
    p.setColor(QPalette::Window, bg);
    p.setColor(QPalette::WindowText, text);
    p.setColor(QPalette::Base, bg);
    p.setColor(QPalette::AlternateBase, alt);
    p.setColor(QPalette::Text, text);
    p.setColor(QPalette::Button, alt);
    p.setColor(QPalette::ButtonText, text);
    p.setColor(QPalette::BrightText, text);
    p.setColor(QPalette::Highlight, c.accent);
    p.setColor(QPalette::HighlightedText, c.accentText);
    p.setColor(QPalette::Link, c.link);
    p.setColor(QPalette::LinkVisited, c.visited);
    p.setColor(QPalette::PlaceholderText, dim);
    p.setColor(QPalette::ToolTipBase, active);
    p.setColor(QPalette::ToolTipText, text);
    p.setColor(QPalette::Light, active);
    p.setColor(QPalette::Midlight, alt);
    p.setColor(QPalette::Mid, active);
    p.setColor(QPalette::Dark, bg.darker(130));
    p.setColor(QPalette::Shadow, Qt::black);
    p.setColor(QPalette::Disabled, QPalette::WindowText, dim);
    p.setColor(QPalette::Disabled, QPalette::Text, dim);
    p.setColor(QPalette::Disabled, QPalette::ButtonText, dim);
    return p;
}
} // namespace

// 1 = Gruvbox dark, 2 = Gruvbox light; anything else leaves the system palette alone.
extern "C" void appofholding_set_palette(int which) {
    if (which == 1) {
        QGuiApplication::setPalette(make(dark));
        // Symbolic icons come in a light set (for dark windows) and a dark set.
        QIcon::setThemeName(QStringLiteral("breeze-dark"));
    } else if (which == 2) {
        QGuiApplication::setPalette(make(light));
        QIcon::setThemeName(QStringLiteral("breeze"));
    }
}

// uQ's theme: `json` is the palette appofholding_gui built from tokens.json (see uqtheme.rs).
// Returns false, changing nothing, when it is unusable.
extern "C" bool appofholding_set_uq_palette(const char *json) {
    const QJsonObject o = QJsonDocument::fromJson(QByteArray(json)).object();
    auto col = [&o](const char *key) { return QColor(o.value(QLatin1String(key)).toString()); };
    Colors c{col("bg"), col("bgAlt"), col("bgActive"), col("text"), col("dim"),
             col("accent"), col("accentText"), col("link"), col("visited")};
    for (const QColor &x : {c.bg, c.bgAlt, c.bgActive, c.text, c.dim, c.accent, c.accentText, c.link, c.visited})
        if (!x.isValid())
            return false;
    const QString wanted = o.value(QLatin1String("icons")).toString();
    const bool light = o.value(QLatin1String("variant")).toString() == QLatin1String("light");
    // Use uQ's icon theme when it is installed here; otherwise Breeze in the matching shade.
    bool found = false;
    if (!wanted.isEmpty())
        for (const QString &dir : QIcon::themeSearchPaths())
            found = found || QFile::exists(dir + QLatin1Char('/') + wanted + QStringLiteral("/index.theme"));
    QGuiApplication::setPalette(make(c));
    QIcon::setThemeName(found ? wanted : light ? QStringLiteral("breeze") : QStringLiteral("breeze-dark"));
    return true;
}
