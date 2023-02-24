trap "exit" INT TERM ERR
trap "kill 0" EXIT

echo "Test: It should watch"
cd ../testrepo

if ../target/release/rewatch clean . &> /dev/null;
then
  echo "✅ - Repo Cleaned"
else 
  echo "❌ - Error Cleaning Repo"
  exit 1
fi

../target/release/rewatch watch . &>/dev/null &
echo "✅ - Watcher Started"

echo 'Js.log("added-by-test")' >> packages/main/src/Main.res

sleep 1

if node ../testrepo/packages/main/src/Main.mjs | grep 'added-by-test' &> /dev/null; 
then
  echo "✅ - Output is correct"
else 
  echo "❌ - Output is incorrect"
  exit 1
fi

sleep 1

sed -i '' '/Js.log("added-by-test")/d' packages/main/src/Main.res;

sleep 1

if git diff --exit-code ./ &> /dev/null; 
then
  echo "✅ - Adding and removing changes nothing"
else 
  echo "❌ - Adding and removing changes left some artifacts"
  exit 1
fi
