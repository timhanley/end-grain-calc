# How the Mac disk image looks when it opens, for dmgbuild
# (https://github.com/dmgbuild/dmgbuild). scripts/bundle-mac.sh --dmg passes
# the app, the background and the volume icon in with -D.
#
# The window is 660 x 460, the size of background.png. Its height includes
# Finder's title bar, so the picture keeps its bottom 40 points plain. The two
# icons sit on the top part of the picture, either side of its arrow.

app = defines["app"]  # noqa: F821 (dmgbuild provides defines)
name = app.rstrip("/").split("/")[-1]

files = [app]
symlinks = {"Applications": "/Applications"}

icon = defines["icon"]  # noqa: F821
background = defines["background"]  # noqa: F821

format = "UDZO"
filesystem = "HFS+"

window_rect = ((200, 140), (660, 460))
default_view = "icon-view"
show_status_bar = False
show_toolbar = False
show_pathbar = False
show_sidebar = False

icon_size = 96
text_size = 13
icon_locations = {
    name: (150, 128),
    "Applications": (510, 128),
}
