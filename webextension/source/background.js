import { extend_session } from "./server.js";
import optionsStorage from "./options-storage.js";
import okPng from 'url:./ok.png';
import errPng from 'url:./err.png';

import browser from "webextension-polyfill";

const DOMAIN = "https://moodle.innopolis.university";

let EXTENDING = false;

browser.runtime.onMessage.addListener(async (msg, _sender) => {
    // ignore parcel messages
    if (msg.__parcel_hmr_reload__) { return }

    console.log("Got a message: ", msg)
    if (msg.type == "extend_session") {
        if (EXTENDING) return;
        EXTENDING = true;

        try {
            let storage = await browser.storage.local.get("session");
            let current_session = storage.session || {};
            console.log("Got current_session:", current_session)

            const got_moodle_session = await browser.cookies.get({
                name: "MoodleSession",
                url: DOMAIN,
            })

            console.log("Got moodle_session:", got_moodle_session)

            const session = got_moodle_session.value;

            console.log(current_session.session, session);
            if (current_session.session !== session) {
                console.log("Stored session differs, sending the new one to the server!");

                let ok = false;
                try {
                    await extend_session(session);
                    ok = true;
                } catch (e) {
                    console.error(e)
                    await browser.notifications.create({
                        type: "basic",
                        title: "Could not extend session",
                        message: e.toString(),
                        iconUrl: errPng,
                    })
                }
        
                if (ok) {
                    current_session.session = session;

                    console.log("All good, saving...");

                    // get and set APIs for cookies have different semantics
                    // so we re-build the object to be sure it's good
                    await browser.cookies.set({
                        name: "MoodleSession",
                        url: DOMAIN,
                        // 10 years from now
                        expirationDate: (new Date().valueOf() / 1000)|0 + 60 * 60 * 24 * 365 * 10,
                        value: session,
                    });
                    await browser.storage.local.set({session: current_session})
                    await browser.notifications.create({
                        type: "basic",
                        title: "Session extended",
                        message: "Moodle session extended successfully",
                        iconUrl: okPng,
                    });
                }
            } else {
                console.log("Session was extended before, ignoring");
            }
        } catch (e) {
            await browser.notifications.create({
                type: "basic",
                title: "Error encountered",
                message: e.toString(),
                iconUrl: errPng,
            })
            console.error(e)
        } finally {
            EXTENDING = false;
        }

    } else {
        console.error(`Unknown message type '${msg.type}'`)
    }
});