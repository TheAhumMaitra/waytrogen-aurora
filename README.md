<h3 align="center">
	<img src="https://github.com/JaKooLit/Telegram-Animated-Emojis/blob/main/Activity/Sparkles.webp" alt="Sparkles" width="38" height="50" />
</h3>

<div align="center">
https://github.com/user-attachments/assets/ebfc8fce-d546-42a8-8ece-a51e66894db5
</div>

---

<div align="center">
A GUI wallpaper setter modified for Aurora that is a spiritual successor for the minimalistic Written purely in the <code>Rust</code>
</div>

## Note
No option for scaling filter in *awww*. Supports each theme's color pallate

## Features
- Recursive and lightning fast file searching
- Can load thousands of wallpapers with ease
- Supports images, GIFs and videos
- Supports external scripts when changing wallpapers
- Can list full wallpaper state in JSON format
- Fully supports:
  - `hyprpaper` (hyprland - png, jpeg, webp, jxl)
  - `swaybg` (sway - png, jpeg, tiff, tga, gif)
  - `mpvpaper` (any video/image format with mpv config)
  - `awww` (jpeg, png, gif, pnm, tga, tiff, webp, bmp, farbfeld with transitions)

## Installation
1. Install required wallpaper changer(s) based on your needs:
    - `hyprpaper` for Hyprland
    - `swaybg` for Sway
    - `mpvpaper` for video support
    - `awww` recommended for Aurora

**Clone thsi repo and install theme by running `cargo install --path .` and lastly move the gschema file and compile it and you are done**

## Usage
- Launch via terminal: `waytrogen`
- Restore previous wallpapers: `waytrogen --restore` or `waytrogen -r`
- List current state in JSON: `waytrogen --list` or `waytrogen -l`
- Use external script: `waytrogen --external_script` or `waytrogen -e`
  - Script receives: monitor, wallpaper path, complete state
  - Overrides `config.json` `executable_script` property. 
- Cycle to the next wallpaper: `waytrogen --next` or `waytrogen -n` 

# License
GPL-3.0-or-later

## Credits
Logo shape from [Inconify Tabler](https://icon-sets.iconify.design/tabler/) atom.
Original work of https://github.com/l3ib/nitrogen
