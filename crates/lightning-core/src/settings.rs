//! Windows Settings pages, launched through the `ms-settings:` URI scheme.

pub struct SettingPage {
    pub title: &'static str,
    pub uri: &'static str,
    /// Extra words that should find this page but aren't in the title.
    pub keywords: &'static str,
}

const fn p(title: &'static str, uri: &'static str, keywords: &'static str) -> SettingPage {
    SettingPage { title, uri, keywords }
}

pub static SETTINGS: &[SettingPage] = &[
    // System
    p("Display", "ms-settings:display", "screen monitor resolution brightness scale hdr night light"),
    p("Night light", "ms-settings:nightlight", "blue light warm display"),
    p("Sound", "ms-settings:sound", "audio volume control speaker microphone output input"),
    p("Volume mixer", "ms-settings:apps-volume", "volume control app sound levels"),
    p("Notifications", "ms-settings:notifications", "alerts toast do not disturb focus"),
    p("Focus", "ms-settings:quiethours", "focus assist do not disturb"),
    p("Power & battery", "ms-settings:powersleep", "sleep battery saver energy shutdown"),
    p("Storage", "ms-settings:storagesense", "disk space cleanup storage sense"),
    p("Nearby sharing", "ms-settings:crossdevice", "share"),
    p("Multitasking", "ms-settings:multitasking", "snap windows alt tab virtual desktops"),
    p("Activation", "ms-settings:activation", "license product key"),
    p("Troubleshoot", "ms-settings:troubleshoot", "fix problems"),
    p("Recovery", "ms-settings:recovery", "reset pc restore advanced startup"),
    p("Projecting to this PC", "ms-settings:project", "miracast wireless display"),
    p("Remote Desktop", "ms-settings:remotedesktop", "rdp remote"),
    p("Clipboard", "ms-settings:clipboard", "copy paste history"),
    p("About", "ms-settings:about", "pc name specs system info version rename"),
    // Bluetooth & devices
    p("Bluetooth & devices", "ms-settings:bluetooth", "pair headphones wireless devices"),
    p("Printers & scanners", "ms-settings:printers", "print scanner"),
    p("Mouse", "ms-settings:mousetouchpad", "pointer cursor scroll speed"),
    p("Touchpad", "ms-settings:devices-touchpad", "gestures trackpad"),
    p("Pen & Windows Ink", "ms-settings:pen", "stylus"),
    p("AutoPlay", "ms-settings:autoplay", "usb removable"),
    p("USB", "ms-settings:usb", "usb devices"),
    p("Camera", "ms-settings:camera", "webcam"),
    // Network
    p("Network & internet", "ms-settings:network-status", "connection internet ethernet"),
    p("Wi-Fi", "ms-settings:network-wifi", "wifi wireless network ssid"),
    p("Ethernet", "ms-settings:network-ethernet", "wired lan"),
    p("VPN", "ms-settings:network-vpn", "virtual private network"),
    p("Mobile hotspot", "ms-settings:network-mobilehotspot", "tethering share internet"),
    p("Airplane mode", "ms-settings:network-airplanemode", "flight mode"),
    p("Proxy", "ms-settings:network-proxy", "proxy server"),
    p("Advanced network settings", "ms-settings:network-advancedsettings", "adapter dns"),
    // Personalization
    p("Personalization", "ms-settings:personalization", "customize look"),
    p("Background", "ms-settings:personalization-background", "wallpaper desktop image"),
    p("Colors", "ms-settings:colors", "dark mode light mode accent color theme"),
    p("Themes", "ms-settings:themes", "theme"),
    p("Lock screen", "ms-settings:lockscreen", "lockscreen"),
    p("Start", "ms-settings:personalization-start", "start menu"),
    p("Taskbar", "ms-settings:taskbar", "tray system tray icons"),
    p("Fonts", "ms-settings:fonts", "typeface"),
    // Apps
    p("Installed apps", "ms-settings:appsfeatures", "uninstall programs apps features remove"),
    p("Default apps", "ms-settings:defaultapps", "browser open with file association"),
    p("Optional features", "ms-settings:optionalfeatures", "features capabilities"),
    p("Startup apps", "ms-settings:startupapps", "launch at login boot"),
    p("Offline maps", "ms-settings:maps", "maps"),
    // Accounts
    p("Your info", "ms-settings:yourinfo", "account profile picture"),
    p("Sign-in options", "ms-settings:signinoptions", "password pin windows hello fingerprint face"),
    p("Email & accounts", "ms-settings:emailandaccounts", "mail accounts"),
    p("Family & other users", "ms-settings:otherusers", "add user accounts"),
    p("Windows backup", "ms-settings:backup", "onedrive sync"),
    // Time & language
    p("Date & time", "ms-settings:dateandtime", "clock time zone"),
    p("Language & region", "ms-settings:regionlanguage", "locale country keyboard input"),
    p("Typing", "ms-settings:typing", "autocorrect spelling keyboard"),
    p("Speech", "ms-settings:speech", "voice recognition"),
    // Gaming
    p("Game Mode", "ms-settings:gaming-gamemode", "gaming performance"),
    p("Game Bar", "ms-settings:gaming-gamebar", "xbox overlay capture"),
    p("Captures", "ms-settings:gaming-gamedvr", "screen recording"),
    // Accessibility
    p("Accessibility", "ms-settings:easeofaccess", "ease of access"),
    p("Text size", "ms-settings:easeofaccess-display", "bigger text"),
    p("Visual effects", "ms-settings:easeofaccess-visualeffects", "animations transparency scrollbars"),
    p("Mouse pointer and touch", "ms-settings:easeofaccess-mousepointer", "cursor size"),
    p("Magnifier", "ms-settings:easeofaccess-magnifier", "zoom"),
    p("Color filters", "ms-settings:easeofaccess-colorfilter", "colorblind grayscale"),
    p("Contrast themes", "ms-settings:easeofaccess-highcontrast", "high contrast"),
    p("Narrator", "ms-settings:easeofaccess-narrator", "screen reader"),
    p("Keyboard accessibility", "ms-settings:easeofaccess-keyboard", "sticky keys on-screen keyboard"),
    // Privacy & security
    p("Privacy & security", "ms-settings:privacy", "permissions"),
    p("Windows Security", "ms-settings:windowsdefender", "antivirus defender firewall virus"),
    p("Find my device", "ms-settings:findmydevice", "lost"),
    p("Location", "ms-settings:privacy-location", "gps"),
    p("Camera permissions", "ms-settings:privacy-webcam", "webcam access"),
    p("Microphone permissions", "ms-settings:privacy-microphone", "mic access"),
    p("For developers", "ms-settings:developers", "developer mode sudo"),
    // Windows Update
    p("Windows Update", "ms-settings:windowsupdate", "updates patch upgrade"),
    p("Update history", "ms-settings:windowsupdate-history", "updates installed"),
    p("Delivery Optimization", "ms-settings:delivery-optimization", "bandwidth"),
];
