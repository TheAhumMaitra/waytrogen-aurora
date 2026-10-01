<h3 align="center">
	<img src="https://github.com/JaKooLit/Telegram-Animated-Emojis/blob/main/Activity/Sparkles.webp" alt="Sparkles" width="38" height="50" />
	    $${\color{red}Waytrogen for Aurora \space \color{lightblue}- \space \color{orange}Wallpaper\space setter\space for\space wayland}$$
	<img src="https://github.com/JaKooLit/Telegram-Animated-Emojis/blob/main/Activity/Sparkles.webp" alt="Sparkles" width="38" height="50" />
</h3>

https://github.com/user-attachments/assets/775c70be-9e70-45eb-99df-869d6ef8c969

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
- Launch with a specific wallpaper folder: `waytrogen open /path/to/the/folder`
  - The folder is canonicalized and stored as the current wallpaper folder
  - Fails without launching the app when the path does not exist or is not a directory
  - Options work before or after the subcommand, so `waytrogen open ~/Pictures --wallust` is valid
- Mix every theme folder together: `waytrogen mixture`
  - Uses every subfolder of every theme in `~/.config/themes` plus `Pictures/Wallpapers`
  - `Pictures` is read from `user-dirs.dirs`, so it follows your XDG user directories
  - The window and `--next`/`--random` all cycle the whole mixture as one list
  - Picking a folder in the window leaves the mixture and goes back to that single folder
  - Options work before or after the subcommand, so `waytrogen mixture --wallust` is valid
  - The mixture needs the `wallpaper-folders` GSettings key, so recompile the schema after updating: `glib-compile-schemas .`
- Restore previous wallpapers: `waytrogen --restore` or `waytrogen -r`
- List current state in JSON: `waytrogen --list` or `waytrogen -l`
- Use external script: `waytrogen --external_script` or `waytrogen -e`
  - Script receives: monitor, wallpaper path, complete state
  - Overrides `config.json` `executable_script` property. 
- Cycle to the next wallpaper: `waytrogen --next` or `waytrogen -n` 
- Generate a theme from the wallpaper before applying it: `waytrogen --wallust`
  - Runs `wallust run <wallpaper>` with the path of the wallpaper being applied, then applies it
  - Runs non interactively on the wallpaper file itself, since wallust only reads a single image and not a folder
  - Can be combined with `open`, `--next`, `--random` and `--restore`

# License
GPL-3.0-or-later

## Credits
Logo shape from [Inconify Tabler](https://icon-sets.iconify.design/tabler/) atom.
Original work of https://github.com/l3ib/nitrogen
