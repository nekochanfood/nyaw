# English Localization Guidelines

This document establishes the copywriting, tone, and terminology standards for nyaw's English UI strings, tutorials, and system messages. It inherits the core user-centric principles from our Japanese guidelines and adapts them for natural, professional English interface design.

---

## 1. Core Principles

### 1.1 User-Centric and Action-Oriented
Frame messages around what the user sees, intends, and accomplishes—never around what the background system is computing. Describe user actions and their immediate outcomes rather than system-level triggers.

- **Avoid (System-centric)**: `Selecting an item causes the window to be mirrored to the dashboard.`
- **Prefer (User-centric)**: `Pick an app or desktop from the list to display it above the dashboard.`

### 1.2 Direct and Confident Action (Will / Imperative over Weak Possibility)
Avoid sprinkling unnecessary "can" or "could" across user actions. State actions cleanly and decisively.

- **Avoid (Weak modal)**: `You can press the button to open settings.`
- **Prefer (Direct)**: `Press the button to open Settings.`

### 1.3 Eliminate Self-Referential "Dev-Talk"
Avoid framing UI elements as substitutes, test stand-ins, or developer commentary. Features should introduce themselves naturally without explaining how the engineering team implemented them.

- **Avoid**: `During the tutorial, a practice window stands in for a real app. Choose Practice window to mirror it.`
- **Prefer**: `Use the practice window to try out navigation and interactions. Select Practice Window to open it on the dashboard.`

---

## 2. Style & Mechanics

### 2.1 Action Sequences: Direct Imperatives
When guiding the user through steps, use clear, connected imperatives. Keep the rhythm brisk and actionable.

- `Point at a screen and squeeze the grip to grab it. Pull toward you to detach it from the dashboard.`
- `Press {button} on the left controller twice to toggle the dashboard.`

### 2.2 Plain UI Terminology over Pipeline Jargon
Internal graphics pipeline, audio engine, or operating system constructs must be translated into clean user-facing concepts.

| Internal / Technical Jargon | Why Avoid | Preferred Term |
| :--- | :--- | :--- |
| **Desktop streaming / Stream** | Easily confused with media broadcasting (Twitch/YouTube) | **Screen mirroring / Mirroring** |
| **Capture source** | Low-level frame-grabber pipeline terminology | **Target window / Source window** |
| **Skeletal input** | Internal bone/joint tracking API name | **Hand tracking** |
| **Hold controller (把持)** | Overly mechanical physical description | **Holding controller / Hand tracking** |
| **config.toml** | Exposes filesystem implementation details | **Settings / Saved automatically** |
| **Child over surface / Far panel** | Scene-graph tree traversal terms | **Surface layer / Background panel** |

### 2.3 Encouraging, Equal Tone
Avoid condescending praise or school-teacher remarks (e.g., "Good boy!", "Nice work!"). Use respectful, satisfying confirmation.

- **Prefer**: `All set!`, `Done!`, `Complete!`

---

## 3. Punctuation & Formatting Conventions

### 3.1 Terminal Periods
- **Use terminal periods**:
  - Full instructional paragraphs or multi-sentence bodies (e.g., tutorial descriptions, multi-step dialog bodies).
- **Omit terminal periods**:
  - Button labels, menu items, table headers, and tab titles.
  - Brief single-sentence alerts, error messages, toast notifications, and status lines.
  - Examples:
    - `Window was minimized` (No period)
    - `Screen capture is not available on this device` (No period)
    - `Notification received` (No period)
    - `All keys are translated` (No period)

### 3.2 Capitalization
- **Sentence case**: Used for titles, section headers, instructions, dialog bodies, and button labels (e.g., `Quick settings`, `Restore window`, `End tutorial`).
- **Brand & Proper Nouns**: Preserve official capitalization (`SteamVR`, `nyaw`, `Windows PowerShell`, `FPS`).

---

## 4. Glossary

| Term (en) | Preferred Usage | Avoid | Context |
| :--- | :--- | :--- | :--- |
| **Mirroring** | Screen mirroring, Window mirroring | Streaming, broadcasting | Projecting desktop/apps into VR |
| **Dashboard** | Dashboard | Dock, bar | The primary floating VR control shelf |
| **Hand tracking** | Hand tracking | Skeletal input, skeleton | Tracking physical hand gestures |
| **Controller** | Controller | Gamepad, wand | Motion controllers |
| **Grab** | Grab | Drag, pick up | Moving floating panels with grip |
| **Detach / Put back** | Detach from dashboard / Put back | Undock / Dock | Separating windows from the shelf |
| **Minimize** | Minimize / Put away | Hide, stash | Reducing a mirrored screen to a pill |
| **Laser** | Laser | Pointer beam, raycast | The pointer line extending from hands |
| **Quick settings** | Quick settings | Control center | Clock-activated control panel |
| **Launcher** | Launcher | App drawer | Menu for choosing windows to mirror |
| **Practice window** | Practice window | Test dummy, mock app | Interactive tutorial panel |
