
import optionsStorage from './options-storage.js';
import browser from "webextension-polyfill";

export async function extend_session(moodle_session) {
    const { server_url } = await optionsStorage.getAll();

    const url = new URL("/extend-session", server_url).toString();

    console.log("Extending session via", url);

    let result;
    const options = {
        method: 'POST',
        headers: {
            'Content-Type': 'application/json'
        },
        body: JSON.stringify({moodle_session})
    };

    const response = await fetch(url, options);
    if (response.status != 200) {
        const msg = `Server replied with some weird status ${response.status}; body = ${await response.text()}`;
        console.error(msg);
        throw msg;
    }
    result = await response.json();
    console.log("Server replied ", result);

    if (!result) {
        throw new Error("Server denied the provided session");
    }



    return true;
}