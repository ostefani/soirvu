export const notificationItems = $state([]);
let notificationSequence = 0;

export function nextNotificationId(sourceId) {
    notificationSequence += 1;
    return `${sourceId}:${notificationSequence}`;
}

export function upsertNotification(notification) {
    const index = notificationItems.findIndex((item) => item.id === notification.id);
    if (index === -1) notificationItems.unshift(notification);
    else notificationItems[index] = { ...notificationItems[index], ...notification };
}

export function removeNotification(id) {
    const index = notificationItems.findIndex((item) => item.id === id);
    if (index !== -1) notificationItems.splice(index, 1);
}

export function clearNotifications() {
    notificationItems.splice(0, notificationItems.length);
}
