/**
 * The devices the LINK pane lists under "Devices on your network".
 *
 * Chris's recorded requirement (issue #582): a device whose ID is above 100
 * (for example TC-SHOWKONTROL) is left out of that list, and 100 and lower
 * stay. It is a display filter only; LINK still sees every device, and the
 * list counts only what it shows.
 */

/** The highest device ID the LINK device list shows. */
export const LINK_DEVICE_LIST_MAX_ID = 100;

/** The devices to list: those whose ID is 100 or lower, in their original order. */
export function listedLinkDevices<T extends { number: number }>(devices: readonly T[]): T[] {
  return devices.filter((device) => device.number <= LINK_DEVICE_LIST_MAX_ID);
}
