import http from "k6/http";
import { check, sleep } from 'k6';

export default function() {
    let response = http.get("LAMBDA_URL");
    check(response, {
        'status is 200': (r) => r.status === 202
    })
    sleep(1)
};
