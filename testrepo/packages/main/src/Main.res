module DuplicatedDep02File = Dep02;

Js.log("01")
Dep01.log()
DuplicatedDep02File.log()

module Array = Belt.Array
module String = Js.String
