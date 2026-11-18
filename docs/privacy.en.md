# Privacy

**English** · [繁體中文](privacy.md)

PaperReader keeps your data on your own device. The core features never touch the network. Only the translation, key-notes, model-download and update-check features you turn on connect to the internet, and each kind of connection is explained first and happens only with your consent. This document lists where the app actually connects and what it sends.

## Where your data lives

- Your library is ordinary folders and files on your device (on Android, by default `Documents/PaperReader`): paper PDFs, translations, key notes, highlights, drawings, notes (Markdown files), reference lists and reading history. You can open them with a file manager or other tools.
- App settings, translation service settings, prompt presets and your glossary are kept in the app-specific storage, not in the library; they are deleted when you uninstall the app.
- Backup files and sync packages are files you export yourself, save where you choose and move yourself. They are unencrypted zip files containing your library (papers, notes, highlights, reading history); anyone who gets the file can open it, so keep them as carefully as the library itself. API keys and app settings are not included.
- The app turns off Android's system backup and device-to-device transfer, so your library is not silently copied to the cloud.
- The project runs no server and has no account system.

## When the app connects to the internet

| Purpose | Connects to | What is sent | Consent first? |
|---|---|---|---|
| Online translation | The service you choose: Anthropic (`api.anthropic.com`), OpenAI (`api.openai.com`), Google Gemini (`generativelanguage.googleapis.com`), DeepL (`api.deepl.com`, or `api-free.deepl.com` for free-tier keys), Google Cloud Translation (`translation.googleapis.com`), or a compatible endpoint address you enter (for example your own Ollama) | The text of the passages to translate, the translation instructions (target language; your glossary terms if you set them; for chat models, also the prompt preset you chose and the terms already settled for this paper), the preceding passage for context, and your API key. No images, notes or highlights | Yes. Before each send, the app shows which service receives what, and sends only if you agree. You can turn off the prompt in settings. Switching to a fallback service always asks first |
| Compare translation services (in Check translation) | The two or three services you pick | The text of that passage or page with the translation instructions, and each service's API key | Yes. Before each send the app shows which services receive it |
| Generate key notes | The chat-model service you choose (Anthropic, OpenAI, Gemini or a compatible endpoint, as above) | The paper's text, the prompt preset you chose, your glossary and the terms already settled for this paper, and your API key | Yes. Before each send the app shows which service receives it, and sends only if you agree |
| Test a service (in settings) | The translation service you configured | One fixed English sentence plus your API key; none of your papers | Only when you press the test button |
| Download an offline translation model | Mozilla's content delivery network, `firefox-settings-attachments.cdn.mozilla.net` | Only a file download request, none of your papers or data; the model is about 34 MB and is verified by hash after download | Yes. The app tells you where it connects and how large the download is, and downloads only if you agree |
| Download the layout model (Settings → Generate) | PaddlePaddle's official model page on Hugging Face, `huggingface.co`, which redirects to Hugging Face's content delivery network (`*.hf.co`) | Only a file download request, none of your papers or data; the app shows the model as about 124 MB, and it is verified by hash after download | Yes. The app tells you where it connects and how large the download is, and downloads only if you agree |
| Check for updates (Android; Settings → About → Check for updates) | GitHub's API, `api.github.com`, asking for this project's (`rainwu0/PaperReader`) release list | One request carrying no data; GitHub sees your IP address and the app name (`PaperReader-update-check`), but not your version, device or data | Yes. Off by default; the first time, the app explains where it connects and what it sends, and checks only if you agree. After that it checks at most once a day when the app opens, and you can turn automatic checks off on the same page, or withdraw your consent (after that no request is sent, and the found version and any downloaded file are deleted) |
| Download an update | GitHub's release page, `github.com`, which redirects to GitHub's file host (currently `release-assets.githubusercontent.com`). The app follows redirects only to `github.com`, `api.github.com` and `*.githubusercontent.com` over https, and stops otherwise | Only file download requests: the installer for this phone's architecture and the checksum list `SHA256SUMS.txt`, with the same header as the update check (the app name) and no `Referer` on redirects. After download the app verifies the SHA-256 and that the installer is signed with the same signing key as the installed app, and deletes it otherwise | Yes. Downloads only after a new version is found and you press Update; on mobile data the app tells you the size first |

Anything not listed above does not use the network. In particular:

- Adding, reading, highlighting, notes, reading history, export, backup and restore, and exporting and importing sync packages all happen on the device.
- After the offline model is downloaded, translation runs on the device and no paper content is sent out.
- The layout model only helps split a paper into paragraphs for translation; it runs on the device and sends no paper content.
- "Organize library" reads each paper on the device to fill in the DOI, arXiv link and year, and to extract the reference list. Matching happens only on this device; it does not use the network or query any bibliographic service.
- The app loads no external fonts, scripts or images; the PDF viewer components ship inside the app.
- There is no analytics, tracking, advertising or crash reporting, no account, and no usage data is collected.

## Android permissions

| Permission | Used for | If not granted |
|---|---|---|
| All files access | Keeping the library in the visible `Documents/PaperReader` folder | The library stays in the app-specific storage and everything else works |
| Network | The purposes in the table above | — |
| Network state | Knowing whether you are on Wi-Fi: "Download on Wi-Fi only", and telling you the size before an update downloads over mobile data | — |
| Install unknown apps (`REQUEST_INSTALL_PACKAGES`) | Updating inside the app: handing the downloaded, verified new version to the system installer. You allow it in system settings; the app takes you there the first time you update in the app | You cannot update inside the app; everything else works, and you can still download the installer from GitHub and install it over the old version |
| Update without confirmation (`UPDATE_PACKAGES_WITHOUT_USER_ACTION`) | From Android 12, lets the system skip the "Install" confirmation when the app updates itself. Granted at install time without a prompt; it only applies to PaperReader's own updates | — |

## API keys

Translation keys are your own accounts and quotas. A key is encrypted with a key generated by the Android Keystore and stored in the app-specific storage; it is not placed in the settings file, the library, backup files or sync packages. A key is sent only to the service you chose.

## How your translation service handles data

Once paper text reaches the service you chose, that provider's privacy policy and data terms apply, and PaperReader cannot control them. If a paper must not leave your device, use offline translation or an endpoint you run yourself.

## Service addresses that start with `http://`

If a service address uses unencrypted `http://` and is not local or on a private network, your key and the paper text are sent in plain text. The app warns you; please use `https://`.

## Changes

This document is updated with the app. Any new network use will be explained here and in the app first, and will need your consent.
