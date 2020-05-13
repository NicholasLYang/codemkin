function binarySearch(list, value, left = 0, right = list.length - 1) {
    if (value > list[right] || value < list[left]) {
        return -1;
    }
    if (value === list[left]) {
        return left;
    }
    if (value === list[right]) {
        return right;
    }
    const mid = Math.floor((left + right)/2);
    if (value === list[mid]) {
        return mid;
    }
    if (mid == left || mid == right) {
        return -1;
    }
    if (list[mid] > value) {
        return binarySearch(list, value, left, mid);
    } else {
        return binarySearch(list, value, mid, right);
    }
}

function randInt(n) {
    return Math.floor(Math.random() * n)
}

function randArray(length, n) {
    const arr = new Set()
    for (let i = 0; i < length; i++) {
	let val = randInt(n)
	while (arr.has(val)) {
	    val = randInt(n)
	}
	arr.add(val)
    }
    return Array.from(arr);
}

for (let i = 0; i < 100; i++) {
    const arr = randArray(10, 100);
    arr.sort((a, b) => {
        if (a > b) {
            return 1;
        }
        if (a < b) {
            return -1;
        }
        return 0;
    });
    const n = randInt(10);
    console.log(`Finding ${arr[n]} in ${arr} (expected ${n})`);
    const out = binarySearch(arr, arr[n])
    if (n !== out) {
	throw new Error(`Wrong output! Expected ${n} got ${out}`)
    } else {
	console.log(out);
    }
}
