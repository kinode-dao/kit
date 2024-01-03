// 240103: Date.now() always returns 0, so this timing does not currently work.

import { printToTerminal, receive, sendResponse } from "uqbar:process/standard@0.5.0";

function parseAddress(addressString) {
    const [node, rest] = addressString.split('@');
    const [process, packageName, publisher] = rest.split(':');
    return { node, process, packageName, publisher };
}

function inputBytesToString(byteObject) {
    // Determine the size of the Uint8Array
    const size = Object.keys(byteObject).length;
    const byteArray = new Uint8Array(size);

    // Assign the bytes to the array
    for (let i = 0; i < size; i++) {
        byteArray[i] = byteObject[i];
    }

    // Convert the Uint8Array to a string
    const string = new TextDecoder().decode(byteArray);

    return {bytes: byteArray, string: string};
}

function fibonacci(n) {
    if (n === 0) return 0;
    if (n === 1) return 1;
    return fibonacci(n - 1) + fibonacci(n - 2);
}

function handleMessage(ourNode) {
    const [source, message] = receive();

    if (message.tag == 'response') {
        printToTerminal(0, `{package_name}: unexpected Response: ${JSON.stringify(message.val)}`);
        process.exit(1);
    } else if (message.tag == 'request') {
        const { bytes: ipcBytes, string: ipc0 } = inputBytesToString(message.val.ipc)
        const ipc = JSON.parse(ipc0);
        const encoder = new TextEncoder();
        if (ipc.Number) {
            const number = ipc.Number;
            const start = Date.now();
            const result = fibonacci(number);
            const duration = (Date.now() - start) * 1000000;
            printToTerminal(0, `{package_name}: fibonacci(${number}) = ${result}; ${duration}ns`);
            sendResponse(
                {
                    inherit: false,
                    ipc: encoder.encode(JSON.stringify({ Number: result })),
                    metadata: null
                },
                null
            );
        } else if (ipc.Numbers) {
            const [number, numberTrials] = ipc.Numbers;
            let durations = [];
            for (let i = 0; i < numberTrials; i++) {
                const start = Date.now();
                const result = fibonacci(number);
                const duration = (Date.now() - start) * 1000000;
                durations.push(duration);
            }
            const result = fibonacci(number);
            const mean = durations.reduce((sum, item) => sum + item, 0) / numberTrials;
            const absoluteDeviation = durations
                .map(item => Math.abs(item - mean))
                .reduce((sum, item) => sum + item, 0) / numberTrials;
            printToTerminal(
                0,
                `{package_name}: fibonacci(${number}) = ${result}; ${mean}±${absoluteDeviation}ns averaged over ${numberTrials} trials`,
            );
            sendResponse(
                {
                    inherit: false,
                    ipc: encoder.encode(JSON.stringify({ Numbers: [result, numberTrials] })),
                    metadata: null
                },
                null
            );
        } else {
            throw new Error(`Unexpected Request: ${ipc}`)
        }
    }
}

export function init(our) {
    printToTerminal(0, `{package_name}: begin (javascript)`);

    const { node: ourNode } = parseAddress(our);

    while (true) {
        try {
            handleMessage(ourNode);
        } catch (error) {
            printToTerminal(0, `{package_name}: got error ${error}`);
        }
    }
}
