project('overskride', 'rust',
          version: '0.6.5',
    meson_version: '>= 0.62.0',
    default_options : 'prefix=/usr',
  default_options: [ 'warning_level=2', 'werror=false', ],
)

i18n = import('i18n')
gnome = import('gnome')

subdir('data')
subdir('src')
subdir('po')

development_environment = environment()
development_environment.set('GSETTINGS_SCHEMA_DIR', meson.project_build_root() / 'data')
meson.add_devenv(development_environment)

gnome.post_install(
     glib_compile_schemas: true,
    gtk_update_icon_cache: true,
  update_desktop_database: true,
)
