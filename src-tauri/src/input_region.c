/* 悬浮窗输入区域：只让顶部控制条可点，其余区域点击穿透。
 *
 * GTK3 的 Wayland 后端没有实现 GdkWindowClass::input_shape_combine_region
 * （gdkwindow-wayland.c 里 input_region_dirty 从未被置位），所以直接用
 * wl_surface.set_input_region 设置。借助头文件生成封送代码，避免手写变参 FFI。
 */
#include <gtk/gtk.h>
#include <gdk/gdkwayland.h>
#include <wayland-client.h>

/* height > 0：只要顶部 height 像素可点；height <= 0：整窗可点（清空限制）。
 * 返回 1 表示已设置，0 表示当前不是 Wayland 会话（调用方退回整窗穿透）。 */
int maku_set_input_region(GdkWindow *window, int height) {
    if (window == NULL) {
        return 0;
    }
    GdkDisplay *display = gdk_window_get_display(window);
    if (display == NULL || !GDK_IS_WAYLAND_DISPLAY(display)) {
        return 0;
    }
    struct wl_surface *surface = gdk_wayland_window_get_wl_surface(window);
    if (surface == NULL) {
        return 0;
    }

    if (height <= 0) {
        /* NULL 区域 = 整窗可交互 */
        wl_surface_set_input_region(surface, NULL);
        wl_surface_commit(surface);
        return 1;
    }

    struct wl_compositor *compositor = gdk_wayland_display_get_wl_compositor(display);
    if (compositor == NULL) {
        return 0;
    }
    int width = gdk_window_get_width(window);
    struct wl_region *region = wl_compositor_create_region(compositor);
    if (region == NULL) {
        return 0;
    }
    wl_region_add(region, 0, 0, width, height);
    wl_surface_set_input_region(surface, region);
    wl_region_destroy(region);
    wl_surface_commit(surface);

    struct wl_display *wl_display = gdk_wayland_display_get_wl_display(display);
    if (wl_display != NULL) {
        wl_display_flush(wl_display);
    }
    return 1;
}
/* 按标题找到悬浮窗顶层窗口，设置它的输入区域。
 * Rust 侧不碰 gtk 类型，只传标题与高度。返回 1 表示已设置。 */
int maku_set_input_region_for_title(const char *title, int height) {
    if (title == NULL) {
        return 0;
    }
    int result = 0;
    GList *tops = gtk_window_list_toplevels();
    for (GList *it = tops; it != NULL; it = it->next) {
        GtkWindow *win = GTK_WINDOW(it->data);
        const char *current = gtk_window_get_title(win);
        if (current == NULL || g_strcmp0(current, title) != 0) {
            continue;
        }
        GdkWindow *window = gtk_widget_get_window(GTK_WIDGET(win));
        result = maku_set_input_region(window, height);
        break;
    }
    g_list_free(tops);
    return result;
}
